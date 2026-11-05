//! Load every STEP file in a folder and compare its bounds with the OBJ of
//! the same name: `cargo run --release -p lamina-step --example corpus -- <dir>`.

use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).expect("folder");
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    let (mut ok, mut total) = (0, 0);
    for p in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "step" || e == "stp"))
    {
        total += 1;
        let bytes = std::fs::read(p).unwrap();
        let t = Instant::now();
        let r = lamina_step::step::load(&bytes);
        let ms = t.elapsed().as_millis();
        let name = p.file_name().unwrap().to_string_lossy();
        match r {
            Ok(m) => {
                let (lo, hi) = m.bounds().unwrap();
                let obj = p.with_extension("obj");
                let cmp = std::fs::read_to_string(&obj)
                    .ok()
                    .and_then(|t| lamina_step::obj::parse(&t).ok())
                    .and_then(|o| o.bounds())
                    .map(|(a, b)| {
                        let size =
                            |l: [f32; 3], h: [f32; 3]| [h[0] - l[0], h[1] - l[1], h[2] - l[2]];
                        let (s1, s2) = (size(lo, hi), size(a, b));
                        let mut s1s = s1;
                        let mut s2s = s2;
                        s1s.sort_by(|x, y| x.total_cmp(y));
                        s2s.sort_by(|x, y| x.total_cmp(y));
                        let d = (0..3).map(|i| (s1s[i] - s2s[i]).abs()).fold(0f32, f32::max);
                        format!(
                            "size {:.2}x{:.2}x{:.2} vs obj {:.2}x{:.2}x{:.2}  max diff {:.3}",
                            s1[0], s1[1], s1[2], s2[0], s2[1], s2[2], d
                        )
                    })
                    .unwrap_or_default();
                ok += 1;
                println!(
                    "ok   {name:<16} {:>6} tris {:>5} ms  {cmp}",
                    m.triangles(),
                    ms
                );
            }
            Err(e) => println!("FAIL {name:<16} {e}"),
        }
    }
    println!("{ok}/{total} loaded");
}
