//! Router benchmark on real boards: strip tracks and vias (zones stay),
//! route again, report completion, vias, length, time and DRC errors.
//!
//! `cargo run --release -p lamina-route --example bench -- <project dir>...`

use std::time::Instant;

use lamina_model::*;

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    for dir in dirs {
        let mut d = ProjectData::load(std::path::Path::new(&dir)).expect("load");
        let before_tracks = d.routing.tracks.len();
        d.routing.tracks.clear();
        d.routing.vias.clear();
        // Poured nets are routed only where the pour does not reach.
        let todo = lamina_drc::analyze(&d).ratsnest.len();
        let opt = lamina_route::RouteOptions {
            time_limit: Some(300.0),
            grid: std::env::var("GRID").ok().and_then(|g| g.parse().ok()),
            ..Default::default()
        };
        let t = Instant::now();
        let out = lamina_route::autoroute(&d, &opt).expect("route");
        let secs = t.elapsed().as_secs_f64();
        let len: f64 = out
            .tracks
            .iter()
            .map(|t| t.points.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>())
            .sum();
        d.routing.tracks.extend(out.tracks.iter().cloned());
        d.routing.vias.extend(out.vias.iter().cloned());
        let a = lamina_drc::analyze(&d);
        // SAVE=<folder>: write the routed board there (a copy of the project).
        if let Ok(to) = std::env::var("SAVE") {
            let to =
                std::path::Path::new(&to).join(std::path::Path::new(&dir).file_name().unwrap());
            d.save(&to, FileKind::Routing).expect("save");
            println!("  saved to {}", to.display());
        }
        let left = a.ratsnest.len();
        let errors: Vec<&Violation> = a
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        println!(
            "{dir}\n  connections {todo}, still open {left}, failed {}, tracks {} (original {before_tracks}), vias {}, length {:.0} mm, {:.1} s, DRC errors {}",
            out.failed.len(),
            out.tracks.len(),
            out.vias.len(),
            len,
            secs,
            errors.len()
        );
        let mut rules: std::collections::BTreeMap<&str, usize> = Default::default();
        for e in &errors {
            *rules.entry(e.rule.as_str()).or_default() += 1;
        }
        if !rules.is_empty() {
            println!("  errors by rule: {rules:?}");
        }
        for e in a.ratsnest.iter().take(8) {
            println!("  open {} {} -> {}", e.net, e.from, e.to);
        }
        for f in out.failed.iter().take(8) {
            println!("  failed {} {} -> {}: {}", f.net, f.from, f.to, f.reason);
        }
    }
}
