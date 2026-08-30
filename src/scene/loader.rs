//! Mesh loading for the scene engine.
//!
//! Wraps [`crate::mesh::io`] with a single entry point that takes raw bytes
//! plus a [`MeshFormat`] enum. Phase-5 cleanup will fold the underlying
//! parsers into this module.

use crate::mesh::io;
use crate::mesh::io::NamedMesh;
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

/// Load a mesh from raw bytes, given an explicit format.
///
/// Container formats that hold several parts (3MF) are **merged** into one
/// mesh. Use [`load_bytes_multi`] to keep the parts separate.
pub fn load_bytes(bytes: &[u8], format: MeshFormat) -> Result<Mesh, String> {
    match format {
        MeshFormat::Stl => io::read_stl_from_bytes(bytes).map_err(|e| e.to_string()),
        MeshFormat::Obj => io::read_obj_from_bytes(bytes).map_err(|e| e.to_string()),
        MeshFormat::Threemf => io::read_3mf_from_bytes(bytes).map_err(|e| e.to_string()),
    }
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
    match format {
        MeshFormat::Threemf => {
            let parts = io::read_3mf_objects_from_bytes(bytes).map_err(|e| e.to_string())?;
            if parts.is_empty() {
                return Err("3MF contains no printable geometry".to_string());
            }
            Ok(parts)
        }
        _ => Ok(vec![NamedMesh {
            name: None,
            mesh: load_bytes(bytes, format)?,
        }]),
    }
}

/// Load every object at `path`, auto-detecting the format from the extension.
///
/// See [`load_bytes_multi`] — this is the on-disk counterpart, used by the
/// server and CLI where meshes are read from files rather than uploads.
pub fn load_path_multi(path: &Path) -> Result<Vec<NamedMesh>, String> {
    match MeshFormat::from_path(path) {
        Some(format @ MeshFormat::Threemf) => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            load_bytes_multi(&bytes, format)
        }
        _ => Ok(vec![NamedMesh {
            name: None,
            mesh: load_path(path)?,
        }]),
    }
}

/// Load a mesh from a path, auto-detecting the format from the extension.
pub fn load_path(path: &Path) -> Result<Mesh, String> {
    io::read_mesh(path).map_err(|e| e.to_string())
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
