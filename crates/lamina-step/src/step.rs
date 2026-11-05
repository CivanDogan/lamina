//! STEP (ISO 10303-21) B-rep reader and tessellator.

use crate::mesh::Mesh;

pub fn load(_bytes: &[u8]) -> Result<Mesh, String> {
    Err("STEP loading is not implemented yet".into())
}
