//! Rendering: CPU rasterizer for 2D snapshots and textures, wgpu for 3D
//! and the live viewer.

pub mod gpu;
pub mod raster;
pub mod tess;

pub use gpu::{Camera, Gpu, Gpu2D, Gpu3D, MeshCache, Target};

use lamina_model::Side;
use lamina_scene::{Scene2D, Scene3D};

/// Snapshot kinds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Snapshot {
    /// Technical layer view.
    Layers,
    /// Finished board seen from one side.
    Side(Side),
    /// Perspective 3D view.
    View3D { yaw: f32, pitch: f32 },
}

/// Render a PNG. 2D views work without a GPU; 3D needs one.
pub fn snapshot(
    s2: &Scene2D,
    s3: &Scene3D,
    kind: Snapshot,
    width: u32,
    height: u32,
    opts2d: Option<&raster::Options2D>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    match kind {
        Snapshot::Layers => {
            let def = raster::Options2D {
                width,
                height,
                layers: None,
                region: None,
            };
            let pm = raster::render_layers(s2, opts2d.unwrap_or(&def)).ok_or("bad image size")?;
            Ok((raster::png(&pm)?, Vec::new()))
        }
        Snapshot::Side(side) => {
            let pm = raster::render_side(s2, side, width, height).ok_or("bad image size")?;
            Ok((raster::png(&pm)?, Vec::new()))
        }
        Snapshot::View3D { yaw, pitch } => {
            let gpu = Gpu::headless()?;
            let mut cache = MeshCache::default();
            let scene = gpu.upload_3d(s3, s2, &mut cache);
            let mut cam = Camera::frame(scene.bounds.0, scene.bounds.1);
            cam.yaw = yaw;
            cam.pitch = pitch;
            let t = gpu.target(width, height);
            gpu.draw_3d(&t, &scene, &cam);
            let rgba = gpu.read_rgba(&t)?;
            let mut pm = tiny_skia::Pixmap::new(width, height).ok_or("bad image size")?;
            pm.data_mut().copy_from_slice(&rgba);
            let problems = scene
                .problems
                .iter()
                .map(|(r, e)| format!("{r}: model not loaded: {e}"))
                .collect();
            Ok((raster::png(&pm)?, problems))
        }
    }
}
