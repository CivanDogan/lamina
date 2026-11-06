//! Print the bounds of a model: `bounds <file>`.
fn main() {
    let p = std::path::PathBuf::from(std::env::args().nth(1).expect("file"));
    let t = std::time::Instant::now();
    let m = lamina_step::load(&p).unwrap();
    let (lo, hi) = m.bounds().unwrap();
    println!(
        "{} tris, {} colours, {} ms",
        m.triangles(),
        m.parts.len(),
        t.elapsed().as_millis()
    );
    println!("min {:.3?}\nmax {:.3?}", lo, hi);
    for part in &m.parts {
        println!(
            "  colour {:.2?}  {} tris",
            part.color,
            part.positions.len() / 3
        );
    }
}
