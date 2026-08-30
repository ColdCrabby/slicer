//! Mesh file reading: STL (binary and ASCII), OBJ (Wavefront), and 3MF.
//!
//! The `stl_io` crate handles STL formats transparently.
//! The `tobj` crate handles OBJ files.
//! 3MF files are ZIP archives containing an XML mesh description, parsed with
//! `zip` and `quick-xml`.
//! Loaded meshes are in native file coordinates — no placement transforms are
//! applied on import, with two 3MF-specific exceptions. First, 3MF's declared
//! measurement `unit` is normalized to millimeters (the engine's canonical
//! unit) on load. Second, a 3MF model is a scene of objects placed by the
//! `<build>` items and assembled from `<components>`; those build-item and
//! component transforms *are* baked in, because they define the object's own
//! internal geometry (each mesh's triangle indices are local to that mesh), not
//! a user placement on the bed.

use std::fs::OpenOptions;
use std::io::Cursor;
use std::path::Path;

use crate::mesh::types::{Face, Mesh, Vertex};

/// File extensions recognised as 3D model files.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["stl", "obj", "3mf"];

/// Load a mesh from a binary or ASCII STL file.
///
/// # Errors
/// Returns an error if the file cannot be opened, is not a valid STL file,
/// or cannot be converted to the internal mesh representation.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use slicer_engine::mesh::io::read_stl;
/// let mesh = read_stl(Path::new("model.stl")).unwrap();
/// ```
pub fn read_stl(path: &Path) -> Result<Mesh, Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| format!("Cannot open STL file '{}': {}", path.display(), e))?;

    let indexed = stl_io::read_stl(&mut file)
        .map_err(|e| format!("Failed to parse STL file '{}': {}", path.display(), e))?;

    // Convert stl_io vertices (f32) to our Vertex type (f64)
    let vertices: Vec<Vertex> = indexed
        .vertices
        .iter()
        .map(|v| Vertex::new(v[0] as f64, v[1] as f64, v[2] as f64))
        .collect();

    // Reconstruct full Face structs from indexed triangles
    let faces: Vec<Face> = indexed
        .faces
        .iter()
        .map(|tri| {
            let v0 = vertices[tri.vertices[0]];
            let v1 = vertices[tri.vertices[1]];
            let v2 = vertices[tri.vertices[2]];

            let normal_vec = tri.normal;
            let normal = if normal_vec[0] != 0.0 || normal_vec[1] != 0.0 || normal_vec[2] != 0.0 {
                Some(Vertex::new(
                    normal_vec[0] as f64,
                    normal_vec[1] as f64,
                    normal_vec[2] as f64,
                ))
            } else {
                None
            };

            Face {
                vertices: [v0, v1, v2],
                normal,
            }
        })
        .collect();

    Ok(Mesh {
        vertices,
        faces,
        aabb: None,
    })
}

/// Load a mesh from raw STL bytes (binary or ASCII).
///
/// Useful when the STL data has already been read into memory (e.g. uploaded
/// over a WebSocket) rather than read from a file path.
///
/// # Errors
/// Returns an error if the bytes are not a valid STL file or cannot be
/// converted to the internal mesh representation.
pub fn read_stl_from_bytes(bytes: &[u8]) -> Result<Mesh, Box<dyn std::error::Error>> {
    let mut cursor = Cursor::new(bytes);

    let indexed = stl_io::read_stl(&mut cursor)
        .map_err(|e| format!("Failed to parse STL from bytes: {}", e))?;

    let vertices: Vec<Vertex> = indexed
        .vertices
        .iter()
        .map(|v| Vertex::new(v[0] as f64, v[1] as f64, v[2] as f64))
        .collect();

    let faces: Vec<Face> = indexed
        .faces
        .iter()
        .map(|tri| {
            let v0 = vertices[tri.vertices[0]];
            let v1 = vertices[tri.vertices[1]];
            let v2 = vertices[tri.vertices[2]];

            let normal_vec = tri.normal;
            let normal = if normal_vec[0] != 0.0 || normal_vec[1] != 0.0 || normal_vec[2] != 0.0 {
                Some(Vertex::new(
                    normal_vec[0] as f64,
                    normal_vec[1] as f64,
                    normal_vec[2] as f64,
                ))
            } else {
                None
            };

            Face {
                vertices: [v0, v1, v2],
                normal,
            }
        })
        .collect();

    Ok(Mesh {
        vertices,
        faces,
        aabb: None,
    })
}

/// Load a mesh from a Wavefront OBJ file.
///
/// Only triangulated meshes are fully supported. Polygonal faces with more
/// than three vertices are triangulated using a simple fan decomposition
/// (the first vertex of the polygon is shared with every subsequent edge).
///
/// # Errors
/// Returns an error if the file cannot be opened or is not a valid OBJ file.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use slicer_engine::mesh::io::read_obj;
/// let mesh = read_obj(Path::new("model.obj")).unwrap();
/// ```
pub fn read_obj(path: &Path) -> Result<Mesh, Box<dyn std::error::Error>> {
    let (models, _materials) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("Failed to parse OBJ file '{}': {}", path.display(), e))?;

    let mut all_vertices: Vec<Vertex> = Vec::new();
    let mut all_faces: Vec<Face> = Vec::new();

    for model in &models {
        let mesh = &model.mesh;
        let base = all_vertices.len();

        // OBJ positions are stored as a flat [x0,y0,z0, x1,y1,z1, …] array
        for chunk in mesh.positions.chunks_exact(3) {
            all_vertices.push(Vertex::new(
                chunk[0] as f64,
                chunk[1] as f64,
                chunk[2] as f64,
            ));
        }

        // Indices are already triangulated (single_index + triangulate = true)
        for tri in mesh.indices.chunks_exact(3) {
            let v0 = all_vertices[base + tri[0] as usize];
            let v1 = all_vertices[base + tri[1] as usize];
            let v2 = all_vertices[base + tri[2] as usize];
            all_faces.push(Face {
                vertices: [v0, v1, v2],
                normal: None,
            });
        }
    }

    Ok(Mesh {
        vertices: all_vertices,
        faces: all_faces,
        aabb: None,
    })
}

/// Load a mesh from raw OBJ bytes.
///
/// # Errors
/// Returns an error if the bytes are not a valid OBJ file or cannot be
/// converted to the internal mesh representation.
pub fn read_obj_from_bytes(bytes: &[u8]) -> Result<Mesh, Box<dyn std::error::Error>> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut buf_reader = std::io::BufReader::new(&mut cursor);

    let (models, _materials) = tobj::load_obj_buf(
        &mut buf_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |_| tobj::load_mtl_buf(&mut std::io::BufReader::new(std::io::Cursor::new([]))),
    )
    .map_err(|e| format!("Failed to parse OBJ from bytes: {}", e))?;

    let mut all_vertices: Vec<Vertex> = Vec::new();
    let mut all_faces: Vec<Face> = Vec::new();

    for model in &models {
        let mesh = &model.mesh;
        let base = all_vertices.len();

        for chunk in mesh.positions.chunks_exact(3) {
            all_vertices.push(Vertex::new(
                chunk[0] as f64,
                chunk[1] as f64,
                chunk[2] as f64,
            ));
        }

        for tri in mesh.indices.chunks_exact(3) {
            let v0 = all_vertices[base + tri[0] as usize];
            let v1 = all_vertices[base + tri[1] as usize];
            let v2 = all_vertices[base + tri[2] as usize];
            all_faces.push(Face {
                vertices: [v0, v1, v2],
                normal: None,
            });
        }
    }

    Ok(Mesh {
        vertices: all_vertices,
        faces: all_faces,
        aabb: None,
    })
}

/// Convert a 3MF `unit` declaration into a millimeter scale factor.
///
/// The 3MF core spec permits `micron`, `millimeter`, `centimeter`, `inch`,
/// `foot`, and `meter`. The engine operates exclusively in millimeters, so every
/// coordinate is multiplied by the returned factor on import. Returns `None` for
/// an unrecognized unit. Comparison is case-insensitive.
fn unit_to_mm_scale(unit: &str) -> Option<f64> {
    match unit.trim().to_ascii_lowercase().as_str() {
        "micron" => Some(0.001),
        "millimeter" => Some(1.0),
        "centimeter" => Some(10.0),
        "inch" => Some(25.4),
        "foot" => Some(304.8),
        "meter" => Some(1000.0),
        _ => None,
    }
}

/// Load a mesh from a 3MF file.
///
/// 3MF is a ZIP archive containing an XML model descriptor at `3D/3dmodel.model`.
/// Only the triangular mesh geometry is extracted; materials and metadata are
/// intentionally ignored (the engine operates on pure geometry).
///
/// # Errors
/// Returns an error if the file cannot be opened, is not a valid 3MF archive,
/// or the embedded XML cannot be parsed.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use slicer_engine::mesh::io::read_3mf;
/// let mesh = read_3mf(Path::new("model.3mf")).unwrap();
/// ```
pub fn read_3mf(path: &Path) -> Result<Mesh, Box<dyn std::error::Error>> {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| format!("Cannot open 3MF file '{}': {}", path.display(), e))?;

    let bytes = std::io::Read::bytes(file)
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|e| format!("Cannot read 3MF file '{}': {}", path.display(), e))?;

    read_3mf_from_bytes(&bytes)
        .map_err(|e| format!("Failed to parse 3MF file '{}': {}", path.display(), e).into())
}

/// A single object's mesh with triangle indices **local** to that mesh.
///
/// 3MF numbers each object's vertices from zero, so these indices are only
/// meaningful relative to `vertices` here — they are rebased onto the merged
/// output during [`instantiate_3mf_object`].
#[derive(Default)]
struct Raw3mfMesh {
    vertices: Vec<Vertex>,
    triangles: Vec<[usize; 3]>,
}

/// A `<component>`: another object referenced with an optional placement.
struct Raw3mfComponent {
    objectid: String,
    transform: glam::DMat4,
}

/// A resolved `<object>`: either mesh geometry, a set of components, or both.
#[derive(Default)]
struct Raw3mfObject {
    /// The optional `name` attribute, used to label the scene object.
    name: Option<String>,
    mesh: Option<Raw3mfMesh>,
    components: Vec<Raw3mfComponent>,
}

/// A `<build><item>`: the top-level placement of an object in the scene.
struct Raw3mfBuildItem {
    objectid: String,
    transform: glam::DMat4,
}

/// Read a named attribute's value as an owned string, if present.
fn attr_str(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find_map(|a| (a.key.local_name().as_ref() == name).then(|| a.value.as_ref().to_owned()))
}

/// Parse a 3MF `transform` attribute (12 row-major floats:
/// `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`) into a column-vector
/// [`glam::DMat4`] such that `M.transform_point3(p)` reproduces the 3MF
/// row-vector product `[x y z 1] · T`.
fn parse_3mf_transform(s: &str) -> Result<glam::DMat4, Box<dyn std::error::Error>> {
    let m: Vec<f64> = s
        .split_whitespace()
        .map(|t| t.parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| "3MF transform contains a non-numeric value")?;
    if m.len() != 12 {
        return Err(format!("3MF transform must have 12 values, got {}", m.len()).into());
    }
    Ok(glam::DMat4::from_cols(
        glam::DVec4::new(m[0], m[1], m[2], 0.0),
        glam::DVec4::new(m[3], m[4], m[5], 0.0),
        glam::DVec4::new(m[6], m[7], m[8], 0.0),
        glam::DVec4::new(m[9], m[10], m[11], 1.0),
    ))
}

/// Recursively bake an object (and its components) into the merged output,
/// applying the accumulated `transform` and `unit_scale`.
///
/// Each mesh's local triangle indices are offset by `base` — the number of
/// vertices already emitted — so concatenating multiple objects never lets one
/// object's triangles reference another object's vertices.
fn instantiate_3mf_object(
    objectid: &str,
    transform: glam::DMat4,
    unit_scale: f64,
    objects: &std::collections::HashMap<String, Raw3mfObject>,
    depth: usize,
    out_vertices: &mut Vec<Vertex>,
    out_faces: &mut Vec<Face>,
) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_DEPTH: usize = 32;
    if depth > MAX_DEPTH {
        return Err("3MF component nesting too deep (possible cyclic reference)".into());
    }

    let obj = objects
        .get(objectid)
        .ok_or_else(|| format!("3MF references unknown object id '{objectid}'"))?;

    if let Some(mesh) = &obj.mesh {
        let base = out_vertices.len();
        for v in &mesh.vertices {
            let p = transform.transform_point3(glam::DVec3::new(v.x, v.y, v.z));
            out_vertices.push(Vertex::new(
                p.x * unit_scale,
                p.y * unit_scale,
                p.z * unit_scale,
            ));
        }
        for &[a, b, c] in &mesh.triangles {
            if a >= mesh.vertices.len() || b >= mesh.vertices.len() || c >= mesh.vertices.len() {
                return Err(format!(
                    "3MF triangle references out-of-bounds vertex index \
                     (v1={a}, v2={b}, v3={c}, vertex count={})",
                    mesh.vertices.len()
                )
                .into());
            }
            out_faces.push(Face {
                vertices: [
                    out_vertices[base + a],
                    out_vertices[base + b],
                    out_vertices[base + c],
                ],
                normal: None,
            });
        }
    }

    for comp in &obj.components {
        instantiate_3mf_object(
            &comp.objectid,
            transform * comp.transform,
            unit_scale,
            objects,
            depth + 1,
            out_vertices,
            out_faces,
        )?;
    }

    Ok(())
}

/// One object resolved out of a container format, with its authored name.
///
/// A 3MF is a *scene*, not a single model: it can place several independent
/// parts on the plate. Keeping them apart lets each become its own scene
/// object — selectable, movable and removable on its own — instead of one
/// fused blob.
#[derive(Debug, Clone)]
pub struct NamedMesh {
    /// The `name` attribute the authoring tool wrote, when present.
    pub name: Option<String>,
    pub mesh: Mesh,
}

/// Load a 3MF's build items as **separate** meshes, in build order.
///
/// Each `<build><item>` becomes one entry with its transform baked in. A model
/// with no `<build>` section falls back to one entry per mesh-bearing
/// `<object>`, preserving the "load all geometry" behaviour.
///
/// Use [`read_3mf_from_bytes`] when a single merged mesh is wanted instead.
///
/// # Errors
/// Returns an error if the bytes are not a valid 3MF archive or the embedded
/// XML cannot be parsed.
pub fn read_3mf_objects_from_bytes(
    bytes: &[u8],
) -> Result<Vec<NamedMesh>, Box<dyn std::error::Error>> {
    let (objects, object_order, build_items, unit_scale) = parse_3mf_model(bytes)?;

    let mut out: Vec<NamedMesh> = Vec::new();

    let mut emit =
        |objectid: &str, transform: glam::DMat4| -> Result<(), Box<dyn std::error::Error>> {
            let mut vertices: Vec<Vertex> = Vec::new();
            let mut faces: Vec<Face> = Vec::new();
            instantiate_3mf_object(
                objectid,
                transform,
                unit_scale,
                &objects,
                0,
                &mut vertices,
                &mut faces,
            )?;
            // An item can resolve to nothing (an object of only empty components);
            // emitting it would put an invisible, unslice-able entry on the plate.
            if faces.is_empty() {
                return Ok(());
            }
            out.push(NamedMesh {
                name: objects.get(objectid).and_then(|o| o.name.clone()),
                mesh: Mesh {
                    vertices,
                    faces,
                    aabb: None,
                },
            });
            Ok(())
        };

    if build_items.is_empty() {
        for id in &object_order {
            if objects.get(id).is_some_and(|o| o.mesh.is_some()) {
                emit(id, glam::DMat4::IDENTITY)?;
            }
        }
    } else {
        for item in &build_items {
            emit(&item.objectid, item.transform)?;
        }
    }

    Ok(out)
}

/// Load a mesh from raw 3MF bytes.
///
/// A 3MF model is a small scene: `<object>` resources hold mesh geometry (with
/// per-object, zero-based vertex indices) or `<components>` that reference other
/// objects with a transform, and `<build><item>` elements place objects into the
/// scene (optionally with their own transform). This resolves that scene into a
/// single merged [`Mesh`], rebasing each object's local triangle indices and
/// baking the build-item and component transforms.
///
/// Callers that want the build items kept apart — so each becomes its own
/// scene object — should use [`read_3mf_objects_from_bytes`] instead.
///
/// # Errors
/// Returns an error if the bytes are not a valid 3MF archive or the embedded
/// XML cannot be parsed.
pub fn read_3mf_from_bytes(bytes: &[u8]) -> Result<Mesh, Box<dyn std::error::Error>> {
    let mut merged = Mesh::new();
    for part in read_3mf_objects_from_bytes(bytes)? {
        // Faces carry their vertices inline, so concatenating is safe — each
        // part already rebased its own indices during instantiation.
        merged.vertices.extend(part.mesh.vertices);
        merged.faces.extend(part.mesh.faces);
    }
    Ok(merged)
}

/// Parse a 3MF archive's model XML into raw objects, their declaration order,
/// the build items, and the unit→mm scale factor.
///
/// This is the "collect pass" shared by the merged and split loaders: it stores
/// elements verbatim (vertices in file coordinates, triangles with local
/// indices) so the resolve pass can apply transforms and rebasing consistently.
#[allow(clippy::type_complexity)]
fn parse_3mf_model(
    bytes: &[u8],
) -> Result<
    (
        std::collections::HashMap<String, Raw3mfObject>,
        Vec<String>,
        Vec<Raw3mfBuildItem>,
        f64,
    ),
    Box<dyn std::error::Error>,
> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use std::collections::HashMap;
    use std::io::Read;

    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Not a valid 3MF (ZIP) file: {}", e))?;

    // Find the primary model file — search for a name ending in .model
    let model_name = (0..archive.len())
        .find_map(|i| {
            archive
                .by_index(i)
                .ok()
                .filter(|f| f.name().ends_with(".model"))
                .map(|f| f.name().to_owned())
        })
        .ok_or("No .model file found inside 3MF archive")?;

    let mut model_file = archive
        .by_name(&model_name)
        .map_err(|e| format!("Cannot open model entry '{}': {}", model_name, e))?;

    let mut xml_content = String::new();
    model_file
        .read_to_string(&mut xml_content)
        .map_err(|e| format!("Cannot read model XML: {}", e))?;

    // Parse the XML using quick-xml
    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(true);

    // ---- Collect pass: gather objects and build items without baking. ----
    let mut objects: HashMap<String, Raw3mfObject> = HashMap::new();
    let mut object_order: Vec<String> = Vec::new();
    let mut build_items: Vec<Raw3mfBuildItem> = Vec::new();

    // The `<model>` element declares the measurement unit for all coordinates.
    // The engine works exclusively in millimeters, so capture the conversion
    // factor (defaults to millimeter per the 3MF spec) and scale every vertex.
    let mut unit_scale = 1.0_f64;

    // Parse context: which `<object>` we are inside, and whether inside `<build>`.
    let mut current_object: Option<String> = None;
    let mut in_build = false;

    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf)?;
        match event {
            // `<object>` and `<build>` open scopes; everything else is a leaf we
            // process identically whether it arrives as a start or empty element.
            Event::Start(ref e) if e.local_name().as_ref() == "object" => {
                if let Some(id) = attr_str(e, "id") {
                    if !objects.contains_key(&id) {
                        object_order.push(id.clone());
                    }
                    let entry = objects.entry(id.clone()).or_default();
                    // Authoring tools label parts here ("top", "bottom", …);
                    // it is the only human-readable handle a 3MF carries, so
                    // it becomes the scene object's display name.
                    if entry.name.is_none() {
                        entry.name = attr_str(e, "name").filter(|n| !n.trim().is_empty());
                    }
                    current_object = Some(id);
                }
            }
            Event::End(ref e) if e.local_name().as_ref() == "object" => {
                current_object = None;
            }
            Event::Start(ref e) if e.local_name().as_ref() == "build" => {
                in_build = true;
            }
            Event::End(ref e) if e.local_name().as_ref() == "build" => {
                in_build = false;
            }
            Event::Start(ref e) | Event::Empty(ref e) => {
                handle_3mf_leaf(
                    e,
                    &mut unit_scale,
                    &mut objects,
                    &mut build_items,
                    current_object.as_deref(),
                    in_build,
                )?;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok((objects, object_order, build_items, unit_scale))
}

/// Process a leaf 3MF element (`model`, `vertex`, `triangle`, `component`,
/// `item`) into the collect-pass state. Elements are stored raw — vertices keep
/// file coordinates and triangles keep local indices — so the resolve pass can
/// apply transforms and rebasing consistently.
fn handle_3mf_leaf(
    e: &quick_xml::events::BytesStart,
    unit_scale: &mut f64,
    objects: &mut std::collections::HashMap<String, Raw3mfObject>,
    build_items: &mut Vec<Raw3mfBuildItem>,
    current_object: Option<&str>,
    in_build: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match e.local_name().as_ref() {
        "model" => {
            if let Some(unit) = attr_str(e, "unit") {
                *unit_scale = unit_to_mm_scale(&unit)
                    .ok_or_else(|| format!("3MF model declares an unknown unit '{unit}'"))?;
            }
        }
        "vertex" => {
            let Some(id) = current_object else {
                return Ok(());
            };
            let mut x = None::<f64>;
            let mut y = None::<f64>;
            let mut z = None::<f64>;
            for attr in e.attributes().flatten() {
                let val: f64 = attr
                    .value
                    .parse()
                    .map_err(|_| "3MF vertex coordinate is not a valid number")?;
                match attr.key.local_name().as_ref() {
                    "x" => x = Some(val),
                    "y" => y = Some(val),
                    "z" => z = Some(val),
                    _ => {}
                }
            }
            let (x, y, z) = match (x, y, z) {
                (Some(x), Some(y), Some(z)) => (x, y, z),
                _ => return Err("3MF vertex is missing x, y, or z attribute".into()),
            };
            objects
                .entry(id.to_owned())
                .or_default()
                .mesh
                .get_or_insert_with(Raw3mfMesh::default)
                .vertices
                .push(Vertex::new(x, y, z));
        }
        "triangle" => {
            let Some(id) = current_object else {
                return Ok(());
            };
            let mut v1 = None::<usize>;
            let mut v2 = None::<usize>;
            let mut v3 = None::<usize>;
            for attr in e.attributes().flatten() {
                let key = attr.key.local_name();
                if !matches!(key.as_ref(), "v1" | "v2" | "v3") {
                    continue;
                }
                let val: usize = attr
                    .value
                    .parse()
                    .map_err(|_| "3MF triangle index is not a valid integer")?;
                match key.as_ref() {
                    "v1" => v1 = Some(val),
                    "v2" => v2 = Some(val),
                    "v3" => v3 = Some(val),
                    _ => {}
                }
            }
            let tri = match (v1, v2, v3) {
                (Some(a), Some(b), Some(c)) => [a, b, c],
                _ => return Err("3MF triangle is missing v1, v2, or v3 attribute".into()),
            };
            objects
                .entry(id.to_owned())
                .or_default()
                .mesh
                .get_or_insert_with(Raw3mfMesh::default)
                .triangles
                .push(tri);
        }
        "component" => {
            let Some(id) = current_object else {
                return Ok(());
            };
            let objectid =
                attr_str(e, "objectid").ok_or("3MF component is missing an objectid attribute")?;
            let transform = match attr_str(e, "transform") {
                Some(t) => parse_3mf_transform(&t)?,
                None => glam::DMat4::IDENTITY,
            };
            objects
                .entry(id.to_owned())
                .or_default()
                .components
                .push(Raw3mfComponent {
                    objectid,
                    transform,
                });
        }
        "item" if in_build => {
            let objectid =
                attr_str(e, "objectid").ok_or("3MF build item is missing an objectid attribute")?;
            let transform = match attr_str(e, "transform") {
                Some(t) => parse_3mf_transform(&t)?,
                None => glam::DMat4::IDENTITY,
            };
            build_items.push(Raw3mfBuildItem {
                objectid,
                transform,
            });
        }
        _ => {}
    }
    Ok(())
}

/// Load a mesh from a file, automatically detecting the format from the file
/// extension.
///
/// Supported extensions (case-insensitive):
/// - `.stl` – STL binary or ASCII
/// - `.obj` – Wavefront OBJ
/// - `.3mf` – 3D Manufacturing Format
///
/// # Errors
/// Returns an error if the format is unsupported, the file cannot be opened,
/// or parsing fails.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use slicer_engine::mesh::io::read_mesh;
/// let mesh = read_mesh(Path::new("model.3mf")).unwrap();
/// ```
pub fn read_mesh(path: &Path) -> Result<Mesh, Box<dyn std::error::Error>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "stl" => read_stl(path),
        "obj" => read_obj(path),
        "3mf" => read_3mf(path),
        other => Err(format!(
            "Unsupported file format '.{}'. Supported: {}",
            other,
            SUPPORTED_EXTENSIONS.join(", ")
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn test_read_binary_stl() {
        let mesh = read_stl(&fixture("simple-cube.stl")).expect("Failed to read binary STL");
        // A unit cube in STL: 12 triangles, 8 unique vertices
        assert_eq!(mesh.faces.len(), 12, "Expected 12 faces");
        assert_eq!(mesh.vertices.len(), 8, "Expected 8 unique vertices");
    }

    #[test]
    fn test_read_ascii_stl() {
        let mesh = read_stl(&fixture("simple-cube-ascii.stl")).expect("Failed to read ASCII STL");
        assert_eq!(mesh.faces.len(), 12, "Expected 12 faces");
        assert_eq!(mesh.vertices.len(), 8, "Expected 8 unique vertices");
    }

    #[test]
    fn test_read_stl_from_bytes() {
        let path = fixture("simple-cube.stl");
        let bytes = std::fs::read(&path).expect("Failed to read fixture bytes");
        let mesh = read_stl_from_bytes(&bytes).expect("Failed to parse STL from bytes");
        assert_eq!(mesh.faces.len(), 12, "Expected 12 faces");
        assert_eq!(mesh.vertices.len(), 8, "Expected 8 unique vertices");
    }

    #[test]
    fn test_read_stl_from_invalid_bytes() {
        let result = read_stl_from_bytes(b"not valid stl data at all");
        assert!(result.is_err(), "Should fail on invalid bytes");
    }

    #[test]
    fn test_missing_file_returns_error() {
        let result = read_stl(Path::new("/nonexistent/path/mesh.stl"));
        assert!(result.is_err(), "Should fail on missing file");
    }

    #[test]
    fn test_invalid_file_returns_error() {
        // Write a temp file with garbage content
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"not an stl file garbage").unwrap();
        let result = read_stl(tmp.path());
        assert!(result.is_err(), "Should fail on invalid STL content");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_obj() {
        let mesh = read_obj(&fixture("simple-cube.obj")).expect("Failed to read OBJ");
        assert_eq!(mesh.faces.len(), 12, "Expected 12 faces");
        assert_eq!(mesh.vertices.len(), 8, "Expected 8 unique vertices");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_obj_missing_file() {
        let result = read_obj(Path::new("/nonexistent/path/mesh.obj"));
        assert!(result.is_err(), "Should fail on missing OBJ file");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf() {
        let mesh = read_3mf(&fixture("simple-cube.3mf")).expect("Failed to read 3MF");
        assert_eq!(mesh.faces.len(), 12, "Expected 12 faces");
        assert_eq!(mesh.vertices.len(), 8, "Expected 8 unique vertices");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_missing_file() {
        let result = read_3mf(Path::new("/nonexistent/path/mesh.3mf"));
        assert!(result.is_err(), "Should fail on missing 3MF file");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_invalid_bytes() {
        let result = read_3mf_from_bytes(b"not a zip archive");
        assert!(result.is_err(), "Should fail on invalid bytes");
    }

    #[test]
    fn test_read_mesh_stl() {
        let mesh = read_mesh(&fixture("simple-cube.stl")).expect("read_mesh should handle STL");
        assert_eq!(mesh.faces.len(), 12);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_mesh_obj() {
        let mesh = read_mesh(&fixture("simple-cube.obj")).expect("read_mesh should handle OBJ");
        assert_eq!(mesh.faces.len(), 12);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_mesh_3mf() {
        let mesh = read_mesh(&fixture("simple-cube.3mf")).expect("read_mesh should handle 3MF");
        assert_eq!(mesh.faces.len(), 12);
    }

    #[test]
    fn test_read_mesh_unsupported_extension() {
        let result = read_mesh(Path::new("model.ply"));
        assert!(result.is_err(), "Should fail on unsupported extension");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unsupported file format"));
    }

    #[test]
    fn test_supported_extensions_contains_known_formats() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"stl"));
        assert!(SUPPORTED_EXTENSIONS.contains(&"obj"));
        assert!(SUPPORTED_EXTENSIONS.contains(&"3mf"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_from_bytes_out_of_bounds_index() {
        use std::io::Write;

        // Build a 3MF in memory with a triangle referencing vertex index 99
        // (but only 3 vertices exist) — should return an error.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="99"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1"/></build>
</model>"#;

        let mut zip_buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zw = zip::ZipWriter::new(cursor);
            zw.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }

        let result = read_3mf_from_bytes(&zip_buf);
        assert!(result.is_err(), "Should fail on out-of-bounds vertex index");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("out-of-bounds"),
            "Error should mention out-of-bounds: {msg}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_3mf_with_unit(unit: &str) -> Vec<u8> {
        use std::io::Write;

        // A single triangle whose far vertex is at coordinate 2 in each axis, so
        // the scale factor is directly observable on the returned geometry.
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="{unit}" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="2" y="0" z="0"/>
          <vertex x="2" y="2" z="2"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1"/></build>
</model>"#
        );

        let mut zip_buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zw = zip::ZipWriter::new(cursor);
            zw.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        zip_buf
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_scales_units_to_millimeters() {
        // (unit, expected mm value for a coordinate of 2 in the source file)
        let cases = [
            ("millimeter", 2.0),
            ("micron", 0.002),
            ("centimeter", 20.0),
            ("inch", 50.8),
            ("foot", 609.6),
            ("meter", 2000.0),
            ("METER", 2000.0), // case-insensitive
        ];
        for (unit, expected) in cases {
            let bytes = build_3mf_with_unit(unit);
            let mesh = read_3mf_from_bytes(&bytes)
                .unwrap_or_else(|e| panic!("Failed to read 3MF with unit '{unit}': {e}"));
            let far = mesh.vertices[1];
            assert!(
                (far.x - expected).abs() < 1e-9,
                "unit '{unit}': expected x={expected}, got {}",
                far.x
            );
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_defaults_to_millimeter_when_unit_absent() {
        use std::io::Write;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="2" y="0" z="0"/>
          <vertex x="2" y="2" z="2"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1"/></build>
</model>"#;
        let mut zip_buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zw = zip::ZipWriter::new(cursor);
            zw.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let mesh = read_3mf_from_bytes(&zip_buf).expect("Failed to read unit-less 3MF");
        assert!((mesh.vertices[1].x - 2.0).abs() < 1e-9);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_rejects_unknown_unit() {
        let bytes = build_3mf_with_unit("furlong");
        let result = read_3mf_from_bytes(&bytes);
        assert!(result.is_err(), "Should fail on unknown unit");
        assert!(result.unwrap_err().to_string().contains("unknown unit"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn zip_model(xml: &str) -> Vec<u8> {
        use std::io::Write;
        let mut zip_buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zw = zip::ZipWriter::new(cursor);
            zw.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        zip_buf
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_multi_object_offsets_local_indices() {
        // Two objects, each with a triangle whose vertex indices restart at 0.
        // Object 2 sits far away (x >= 100). The parser must rebase object 2's
        // local indices onto the merged vertex list; the pre-fix code treated
        // them as global indices, collapsing object 2 onto object 1's vertices.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
    <object id="2" type="model">
      <mesh>
        <vertices>
          <vertex x="100" y="0" z="0"/>
          <vertex x="101" y="0" z="0"/>
          <vertex x="100" y="1" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build>
    <item objectid="1"/>
    <item objectid="2"/>
  </build>
</model>"#;

        let mesh = read_3mf_from_bytes(&zip_model(xml)).expect("Failed to read multi-object 3MF");
        assert_eq!(mesh.vertices.len(), 6, "both objects' vertices merged");
        assert_eq!(mesh.faces.len(), 2, "one face per object");

        // The second face belongs to object 2 and must reference its own far
        // vertices, not object 1's near vertices.
        assert!(
            mesh.faces[1].vertices.iter().all(|v| v.x >= 100.0),
            "object 2's triangle must use object 2's vertices, got {:?}",
            mesh.faces[1].vertices
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_bakes_build_item_transform() {
        // A build item may place its object with a transform (row-major 3x4).
        // Here: identity rotation + translation (10, 20, 30).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="0" z="0"/>
          <vertex x="0" y="0" z="1"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build>
    <item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 20 30"/>
  </build>
</model>"#;

        let mesh = read_3mf_from_bytes(&zip_model(xml)).expect("Failed to read transformed 3MF");
        let v0 = mesh.vertices[0]; // originally (1, 0, 0)
        assert!(
            (v0.x - 11.0).abs() < 1e-9 && (v0.y - 20.0).abs() < 1e-9 && (v0.z - 30.0).abs() < 1e-9,
            "expected (11, 20, 30), got ({}, {}, {})",
            v0.x,
            v0.y,
            v0.z
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_composes_component_and_item_transforms() {
        // Object 2 is an assembly that references object 1 (a mesh) with a
        // component transform (+5 in x). The build item places object 2 with a
        // further transform (+7 in y). The vertex (1,2,3) must end at (6,9,3).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="1" y="2" z="3"/>
          <vertex x="1" y="2" z="4"/>
          <vertex x="1" y="3" z="3"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
    <object id="2" type="model">
      <components>
        <component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
      </components>
    </object>
  </resources>
  <build>
    <item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 7 0"/>
  </build>
</model>"#;

        let mesh = read_3mf_from_bytes(&zip_model(xml)).expect("Failed to read component 3MF");
        assert_eq!(mesh.vertices.len(), 3, "component's mesh baked once");
        let v0 = mesh.vertices[0]; // (1,2,3) -> +5x (component) -> +7y (item)
        assert!(
            (v0.x - 6.0).abs() < 1e-9 && (v0.y - 9.0).abs() < 1e-9 && (v0.z - 3.0).abs() < 1e-9,
            "expected (6, 9, 3), got ({}, {}, {})",
            v0.x,
            v0.y,
            v0.z
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_read_3mf_without_build_merges_all_mesh_objects() {
        // A model with no <build> section still loads every mesh object at
        // identity (backward compatibility with the pre-scene loader).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
</model>"#;

        let mesh = read_3mf_from_bytes(&zip_model(xml)).expect("Failed to read build-less 3MF");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.faces.len(), 1);
    }

    #[test]
    fn test_unit_to_mm_scale() {
        assert_eq!(unit_to_mm_scale("millimeter"), Some(1.0));
        assert_eq!(unit_to_mm_scale("micron"), Some(0.001));
        assert_eq!(unit_to_mm_scale("centimeter"), Some(10.0));
        assert_eq!(unit_to_mm_scale("inch"), Some(25.4));
        assert_eq!(unit_to_mm_scale("foot"), Some(304.8));
        assert_eq!(unit_to_mm_scale("meter"), Some(1000.0));
        assert_eq!(unit_to_mm_scale("MilliMeter"), Some(1.0));
        assert_eq!(unit_to_mm_scale("parsec"), None);
    }
}

#[cfg(test)]
mod multi_object_3mf_tests {
    use super::*;

    /// `TopAC.3mf` (courtesy of @max-scopp) holds two named build items —
    /// "top" and "bottom". It is the regression fixture for a real-world
    /// multi-object 3MF landing on the plate as separate objects.
    fn top_ac_bytes() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/TopAC.3mf"
        ))
        .expect("TopAC.3mf fixture is missing")
    }

    #[test]
    fn splits_real_multi_object_3mf_into_named_parts() {
        let parts = read_3mf_objects_from_bytes(&top_ac_bytes()).expect("3MF should load");

        assert_eq!(parts.len(), 2, "expected the two build items to stay apart");
        assert_eq!(
            parts.iter().map(|p| p.name.as_deref()).collect::<Vec<_>>(),
            vec![Some("top"), Some("bottom")],
        );
        for part in &parts {
            assert!(!part.mesh.faces.is_empty(), "each part must carry geometry");
        }
    }

    #[test]
    fn split_parts_together_equal_the_merged_mesh() {
        // The split loader must not lose or duplicate geometry: merging its
        // output has to reproduce exactly what the single-mesh loader returns.
        let bytes = top_ac_bytes();
        let merged = read_3mf_from_bytes(&bytes).expect("merged load");
        let parts = read_3mf_objects_from_bytes(&bytes).expect("split load");

        let split_faces: usize = parts.iter().map(|p| p.mesh.faces.len()).sum();
        let split_verts: usize = parts.iter().map(|p| p.mesh.vertices.len()).sum();
        assert_eq!(split_faces, merged.faces.len());
        assert_eq!(split_verts, merged.vertices.len());
    }

    #[test]
    fn parts_occupy_distinct_space() {
        // Two separate parts that reported identical bounds would mean the
        // per-item transform was dropped — the scrambling bug in a new guise.
        let parts = read_3mf_objects_from_bytes(&top_ac_bytes()).expect("3MF should load");
        let boxes: Vec<_> = parts
            .iter()
            .map(|p| crate::mesh::analysis::calculate_aabb(&p.mesh))
            .collect();
        assert_ne!(
            (boxes[0].min.z, boxes[0].max.z),
            (boxes[1].min.z, boxes[1].max.z),
            "the 'top' and 'bottom' parts should not share a Z span",
        );
    }
}
