//! Mesh loading for the scene engine.
//!
//! Wraps [`crate::mesh::io`] with a single entry point that takes raw bytes
//! plus a [`MeshFormat`] enum. Phase-5 cleanup will fold the underlying
//! parsers into this module.
//!
//! # Repair is part of loading
//!
//! Every runtime reaches a mesh through this module — the CLI, the WS server,
//! the wasm `SceneHandle`, and the Tauri bridge — so it is the one place a
//! validation/repair pass belongs. [`crate::mesh::repair`] runs by default;
//! a clean mesh is returned untouched (see the no-op contract there), so this
//! costs one analysis pass and changes nothing for well-formed models.

use crate::mesh::io;
use crate::mesh::io::NamedMesh;
use crate::mesh::repair::{self, MeshReport, RepairOptions};
use crate::mesh::types::Mesh;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported mesh file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MeshFormat {
    /// STL (binary or ASCII).
    Stl,
    /// Wavefront OBJ.
    Obj,
    /// 3D Manufacturing Format (3MF).
    Threemf,
}

impl MeshFormat {
    /// Infer the format from a file extension (case-insensitive).
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "stl" => Some(Self::Stl),
            "obj" => Some(Self::Obj),
            "3mf" => Some(Self::Threemf),
            _ => None,
        }
    }

    /// Infer the format from a path's extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    /// Canonical lowercase name (e.g. `"stl"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stl => "stl",
            Self::Obj => "obj",
            Self::Threemf => "3mf",
        }
    }
}

/// One object loaded from a source file, with the health report produced when
/// it was validated.
///
/// The reporting counterpart of [`crate::mesh::io::NamedMesh`].
#[derive(Debug, Clone)]
pub struct LoadedPart {
    /// The part's own name inside the file, when it declares one.
    pub name: Option<String>,
    /// The (possibly repaired) mesh.
    pub mesh: Mesh,
    /// What the repair pass found and did.
    pub report: MeshReport,
}

/// Load a mesh from raw bytes, given an explicit format.
///
/// Container formats that hold several parts (3MF) are **merged** into one
/// mesh. Use [`load_bytes_multi`] to keep the parts separate.
///
/// Defective meshes are repaired on the way in — see
/// [`load_bytes_reporting`] when you also want the health report.
pub fn load_bytes(bytes: &[u8], format: MeshFormat) -> Result<Mesh, String> {
    load_bytes_reporting(bytes, format, &RepairOptions::default()).map(|(mesh, _)| mesh)
}

/// Load every object a file contains, keeping multi-part files apart.
///
/// A 3MF is a scene, not a model: it can place several independent parts on
/// the plate, and fusing them into one mesh makes them impossible to select,
/// move or remove individually. STL and OBJ carry no such structure, so they
/// always yield exactly one entry.
///
/// Always returns at least one entry; a file that resolves to no geometry is
/// an error rather than an empty plate.
pub fn load_bytes_multi(bytes: &[u8], format: MeshFormat) -> Result<Vec<NamedMesh>, String> {
    Ok(load_bytes_multi_reporting(bytes, format, &RepairOptions::default())?
        .into_iter()
        .map(|part| NamedMesh {
            name: part.name,
            mesh: part.mesh,
        })
        .collect())
}

/// Load every object a file contains, each with its own health report.
///
/// Every part is validated and repaired individually — a 3MF's parts are
/// independent models, so one being defective says nothing about the others.
pub fn load_bytes_multi_reporting(
    bytes: &[u8],
    format: MeshFormat,
    options: &RepairOptions,
) -> Result<Vec<LoadedPart>, String> {
    match format {
        MeshFormat::Threemf => {
            let parts = io::read_3mf_objects_from_bytes(bytes).map_err(|e| e.to_string())?;
            if parts.is_empty() {
                return Err("3MF contains no printable geometry".to_string());
            }
            Ok(parts
                .into_iter()
                .map(|part| {
                    let (mesh, report) = finish(part.mesh, options);
                    LoadedPart {
                        name: part.name,
                        mesh,
                        report,
                    }
                })
                .collect())
        }
        _ => {
            let (mesh, report) = load_bytes_reporting(bytes, format, options)?;
            Ok(vec![LoadedPart {
                name: None,
                mesh,
                report,
            }])
        }
    }
}

/// Load every object at `path`, auto-detecting the format from the extension.
///
/// See [`load_bytes_multi`] — this is the on-disk counterpart, used by the
/// server and CLI where meshes are read from files rather than uploads.
pub fn load_path_multi(path: &Path) -> Result<Vec<NamedMesh>, String> {
    Ok(load_path_multi_reporting(path, &RepairOptions::default())?
        .into_iter()
        .map(|part| NamedMesh {
            name: part.name,
            mesh: part.mesh,
        })
        .collect())
}

/// Load every object at `path`, each with its own health report.
///
/// See [`load_bytes_multi_reporting`] — this is the on-disk counterpart.
pub fn load_path_multi_reporting(
    path: &Path,
    options: &RepairOptions,
) -> Result<Vec<LoadedPart>, String> {
    match MeshFormat::from_path(path) {
        Some(format @ MeshFormat::Threemf) => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            load_bytes_multi_reporting(&bytes, format, options)
        }
        _ => {
            let (mesh, report) = load_path_reporting(path, options)?;
            Ok(vec![LoadedPart {
                name: None,
                mesh,
                report,
            }])
        }
    }
}

/// Load a mesh from a path, auto-detecting the format from the extension.
///
/// Defective meshes are repaired on the way in — see
/// [`load_path_reporting`] when you also want the health report.
pub fn load_path(path: &Path) -> Result<Mesh, String> {
    load_path_reporting(path, &RepairOptions::default()).map(|(mesh, _)| mesh)
}

/// Load a mesh from raw bytes and report its topological health.
///
/// With [`RepairOptions::analysis_only`] the mesh is measured but returned
/// verbatim.
pub fn load_bytes_reporting(
    bytes: &[u8],
    format: MeshFormat,
    options: &RepairOptions,
) -> Result<(Mesh, MeshReport), String> {
    let raw = match format {
        MeshFormat::Stl => io::read_stl_from_bytes(bytes).map_err(|e| e.to_string())?,
        MeshFormat::Obj => io::read_obj_from_bytes(bytes).map_err(|e| e.to_string())?,
        MeshFormat::Threemf => io::read_3mf_from_bytes(bytes).map_err(|e| e.to_string())?,
    };
    Ok(finish(raw, options))
}

/// Load a mesh from a path and report its topological health.
pub fn load_path_reporting(
    path: &Path,
    options: &RepairOptions,
) -> Result<(Mesh, MeshReport), String> {
    let raw = io::read_mesh(path).map_err(|e| e.to_string())?;
    Ok(finish(raw, options))
}

/// Run the repair pass and unwrap the `Cow` — a clean mesh is moved out
/// untouched, a repaired one replaces it.
fn finish(raw: Mesh, options: &RepairOptions) -> (Mesh, MeshReport) {
    let (repaired, report) = repair::repair(&raw, options);
    match repaired {
        std::borrow::Cow::Borrowed(_) => (raw, report),
        std::borrow::Cow::Owned(mesh) => (mesh, report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_extension_is_case_insensitive() {
        assert_eq!(MeshFormat::from_extension("STL"), Some(MeshFormat::Stl));
        assert_eq!(MeshFormat::from_extension("3mf"), Some(MeshFormat::Threemf));
        assert_eq!(MeshFormat::from_extension("xyz"), None);
    }

    #[test]
    fn format_from_path_uses_extension() {
        assert_eq!(
            MeshFormat::from_path(Path::new("/tmp/cube.OBJ")),
            Some(MeshFormat::Obj)
        );
    }
}
