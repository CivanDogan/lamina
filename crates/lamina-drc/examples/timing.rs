//! Measure the time of each analysis step on a project: `cargo run --release -p lamina-drc --example timing -- <dir>`.
use std::time::Instant;

fn main() {
    for dir in std::env::args().skip(1) {
        let t = Instant::now();
        let d = lamina_model::ProjectData::load(std::path::Path::new(&dir)).unwrap();
        let load = t.elapsed();
        let t = Instant::now();
        let g = lamina_geom::build(&d);
        let build = t.elapsed();
        let t = Instant::now();
        let f = lamina_drc::fills(&d, &g);
        let fills = t.elapsed();
        let t = Instant::now();
        let a = lamina_drc::analyze(&d);
        let all = t.elapsed();
        println!(
            "{dir}: load {load:?}, geometry {build:?}, fills {fills:?} ({} polys), analyze {all:?}, {} violations",
            f.iter().map(|z| z.polygons.len()).sum::<usize>(),
            a.violations.len()
        );
    }
}
