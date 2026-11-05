//! Render one model file to PNG: `meshview <model> <out.png> [yaw] [pitch]`.

use lamina_model::{Point, Rect};
use lamina_render::{Camera, Gpu, MeshCache};
use lamina_scene::{Instance, Scene2D, Scene3D};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let model = std::path::PathBuf::from(&a[1]);
    let yaw: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(-55.0);
    let pitch: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let id = [
        1.0, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    let s3 = Scene3D {
        outline: vec![],
        holes: vec![],
        thickness: 0.0,
        instances: vec![Instance {
            refdes: "X".into(),
            model: Some(model),
            transform: id,
            placeholder: None,
        }],
        bbox: Rect {
            min: Point::new(-1.0, -1.0),
            max: Point::new(1.0, 1.0),
        },
    };
    let s2 = Scene2D {
        layers: vec![],
        bbox: s3.bbox,
        copper_layers: 2,
        mask_color: "green".into(),
        silk_color: "white".into(),
        finish: "hasl".into(),
    };
    let gpu = Gpu::headless().unwrap();
    let mut cache = MeshCache::default();
    let g = gpu.upload_3d(&s3, &s2, &mut cache);
    for p in &g.problems {
        eprintln!("problem: {p:?}");
    }
    let mut cam = Camera::frame(g.bounds.0, g.bounds.1);
    cam.yaw = yaw;
    cam.pitch = pitch;
    let t = gpu.target(900, 700);
    gpu.draw_3d(&t, &g, &cam);
    let rgba = gpu.read_rgba(&t).unwrap();
    let mut pm = tiny_skia::Pixmap::new(900, 700).unwrap();
    pm.data_mut().copy_from_slice(&rgba);
    std::fs::write(&a[2], pm.encode_png().unwrap()).unwrap();
}
