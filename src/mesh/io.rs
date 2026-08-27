//! Mesh file reading: STL (binary and ASCII), OBJ (Wavefront), and 3MF.
//!
//! The `stl_io` crate handles STL formats transparently.
//! The `tobj` crate handles OBJ files.
//! 3MF files are ZIP archives containing an XML mesh description, parsed with
//! `zip` and `quick-xml`.
//! Loaded meshes are in native file coordinates — no placement transforms are
//! applied on import. The one exception is 3MF's declared measurement `unit`,
//! which is normalized to millimeters (the engine's canonical unit) on load.

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

/// Load a mesh from raw 3MF bytes.
///
/// # Errors
/// Returns an error if the bytes are not a valid 3MF archive or the embedded
/// XML cannot be parsed.
pub fn read_3mf_from_bytes(bytes: &[u8]) -> Result<Mesh, Box<dyn std::error::Error>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
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

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut faces: Vec<Face> = Vec::new();
    let mut buf = Vec::new();

    // The `<model>` element declares the measurement unit for all coordinates.
    // The engine works exclusively in millimeters, so capture the conversion
    // factor (defaults to millimeter per the 3MF spec) and scale every vertex.
    // `<model>` always precedes the mesh data, so this is known before any vertex.
    let mut unit_scale = 1.0_f64;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Empty(ref e) | Event::Start(ref e) => match e.local_name().as_ref() {
                "model" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == "unit" {
                            let unit = attr.value.as_ref();
                            unit_scale = unit_to_mm_scale(unit).ok_or_else(|| {
                                format!("3MF model declares an unknown unit '{unit}'")
                            })?;
                        }
                    }
                }
                "vertex" => {
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
                    vertices.push(Vertex::new(x * unit_scale, y * unit_scale, z * unit_scale));
                }
                "triangle" => {
                    let mut v1 = None::<usize>;
                    let mut v2 = None::<usize>;
                    let mut v3 = None::<usize>;
                    for attr in e.attributes().flatten() {
                        let val: usize = attr
                            .value
                            .parse()
                            .map_err(|_| "3MF triangle index is not a valid integer")?;
                        match attr.key.local_name().as_ref() {
                            "v1" => v1 = Some(val),
                            "v2" => v2 = Some(val),
                            "v3" => v3 = Some(val),
                            _ => {}
                        }
                    }
                    let (v1, v2, v3) = match (v1, v2, v3) {
                        (Some(a), Some(b), Some(c)) => (a, b, c),
                        _ => return Err("3MF triangle is missing v1, v2, or v3 attribute".into()),
                    };
                    if v1 >= vertices.len() || v2 >= vertices.len() || v3 >= vertices.len() {
                        return Err(format!(
                            "3MF triangle references out-of-bounds vertex index \
                             (v1={v1}, v2={v2}, v3={v3}, vertex count={})",
                            vertices.len()
                        )
                        .into());
                    }
                    faces.push(Face {
                        vertices: [vertices[v1], vertices[v2], vertices[v3]],
                        normal: None,
                    });
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(Mesh {
        vertices,
        faces,
        aabb: None,
    })
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
