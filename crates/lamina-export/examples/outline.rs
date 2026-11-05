//! Print the closed loops found in a Gerber outline layer, for debugging imports.
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: outline <file.gbr> [tolerance]");
    let tol: f64 = std::env::args()
        .nth(2)
        .and_then(|t| t.parse().ok())
        .unwrap_or(0.02);
    let text = std::fs::read_to_string(&path).unwrap();
    let g = lamina_export::gerber_in::read(&text).unwrap();
    println!("{} edges", g.edges.len());
    let (loops, open) = lamina_model::chain_edges(g.edges, tol);
    println!("{} loops, {open} open", loops.len());
    for c in &loops {
        let b = c.bbox().unwrap();
        println!(
            "  {} vertices, area {:.2}, bbox {:.2},{:.2} .. {:.2},{:.2}, valid {:?}",
            c.vertices.len(),
            c.signed_area(),
            b.min.x,
            b.min.y,
            b.max.x,
            b.max.y,
            c.check().is_ok()
        );
    }
}
