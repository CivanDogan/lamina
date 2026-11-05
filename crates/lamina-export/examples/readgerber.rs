//! Print the dark area of a Gerber file: `readgerber <file>`.
use geo::Area;
fn main() {
    let p = std::env::args().nth(1).expect("file");
    let t = std::fs::read_to_string(&p).unwrap();
    let m = lamina_export::reader::read_gerber(&t).unwrap();
    println!("{} polygons, {:.1} mm2", m.0.len(), m.unsigned_area());
}
