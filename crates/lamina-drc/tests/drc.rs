use lamina_drc::*;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    for pkg in ["R_0603", "PinHeader_1x03_P2.54"] {
        let fp = from_package(pkg, &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp);
    }
    d.parts.parts.insert("R1".into(), Part::new("R_0603"));
    d.parts.parts.insert("R2".into(), Part::new("R_0603"));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 10.0));
    d.placement
        .placements
        .insert("R2".into(), Placement::at(20.0, 10.0));
    d.nets.nets.insert(
        "A".into(),
        Net {
            pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
            class: None,
        },
    );
    d.nets.nets.insert(
        "B".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap()],
            class: None,
        },
    );
    d
}

fn rules(a: &Analysis) -> Vec<&str> {
    a.violations.iter().map(|v| v.rule.as_str()).collect()
}

#[test]
fn unrouted_then_routed() {
    let mut d = board();
    let a = analyze(&d);
    assert!(rules(&a).contains(&"unrouted"), "{:?}", a.violations);
    assert_eq!(a.ratsnest.len(), 1);
    assert_eq!(a.ratsnest[0].net, "A");
    let r1p2 = d.footprints["R_0603"]
        .pad("2")
        .unwrap()
        .at
        .add(Point::new(10.0, 10.0));
    let r2p1 = d.footprints["R_0603"]
        .pad("1")
        .unwrap()
        .at
        .add(Point::new(20.0, 10.0));
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![r1p2, r2p1],
    });
    let a = analyze(&d);
    assert!(a.ratsnest.is_empty(), "{:?}", a.ratsnest);
    assert!(!rules(&a).contains(&"unrouted"));
    assert!(
        a.violations.iter().all(|v| v.severity != Severity::Error),
        "{:?}",
        a.violations
    );
}

#[test]
fn clearance_and_short() {
    let mut d = board();
    // Track of net B passing 0.05 mm above R2's pad 1 (net A).
    let p = d.footprints["R_0603"].pad("1").unwrap().clone();
    let top = 10.0 + p.shape.size().1 / 2.0;
    d.routing.tracks.push(Track {
        id: "t9".into(),
        net: "B".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(15.0, top + 0.15), Point::new(25.0, top + 0.15)],
    });
    let a = analyze(&d);
    assert!(rules(&a).contains(&"clearance"), "{:?}", a.violations);
    // Move it onto the pad: short.
    d.routing.tracks[0].points = vec![Point::new(15.0, 10.0), Point::new(25.0, 10.0)];
    let a = analyze(&d);
    assert!(rules(&a).contains(&"short"), "{:?}", a.violations);
    assert!(rules(&a).contains(&"short_nets"), "{:?}", a.violations);
}

#[test]
fn edge_courtyard_drill() {
    let mut d = board();
    d.placement
        .placements
        .insert("R2".into(), Placement::at(10.5, 10.0));
    d.parts
        .parts
        .insert("J1".into(), Part::new("PinHeader_1x03_P2.54"));
    d.placement
        .placements
        .insert("J1".into(), Placement::at(0.2, 5.0));
    let a = analyze(&d);
    let r = rules(&a);
    assert!(r.contains(&"courtyard_overlap"), "{r:?}");
    assert!(
        r.contains(&"copper_outside_board") || r.contains(&"copper_to_edge"),
        "{r:?}"
    );
    d.board.rules.min_drill = 1.2;
    let a = analyze(&d);
    assert!(rules(&a).contains(&"drill_size"));
}

#[test]
fn via_rules() {
    let mut d = board();
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(5.0, 5.0),
        diameter: 0.45,
        drill: 0.25,
        from: None,
        to: None,
    });
    let a = analyze(&d);
    let r = rules(&a);
    assert!(
        r.contains(&"via_drill") && r.contains(&"annular_ring"),
        "{r:?}"
    );
}

#[test]
fn pour_connects_pads() {
    let mut d = board();
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(0.0, 0.0, 30.0, 20.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    assert_eq!(a.fills.len(), 1);
    assert!(!a.fills[0].polygons.is_empty());
    assert!(
        a.ratsnest.is_empty(),
        "pour should connect R1.2 and R2.1: {:?}",
        a.ratsnest
    );
    assert!(
        a.violations.iter().all(|v| v.severity != Severity::Error),
        "{:?}",
        a.violations
    );
    // The fill keeps clear of net B's pad.
    let b_pad = lamina_geom::pad_prim(
        d.footprints["R_0603"].pad("1").unwrap(),
        &d.placement.placements["R1"],
    );
    for p in &a.fills[0].polygons {
        assert!(
            p.prim().distance(&b_pad) >= 0.149,
            "{}",
            p.prim().distance(&b_pad)
        );
    }
}

#[test]
fn presets_exist() {
    assert!(presets::get("jlcpcb-2layer").is_some());
    assert!(presets::get("nope").is_none());
}

#[test]
fn two_pours_of_a_net_join_where_they_overlap() {
    let zone = |id: &str, x: f64, w: f64| Zone {
        id: id.into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(x, 5.0, w, 10.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    };
    // R1.2 is near x 10.8, R2.1 near x 19.2: one pour around each pad.
    let mut d = board();
    d.routing.zones.push(zone("left", 10.5, 5.0));
    d.routing.zones.push(zone("right", 14.0, 6.0));
    let a = analyze(&d);
    assert!(
        a.ratsnest.is_empty(),
        "overlapping pours join: {:?}",
        a.ratsnest
    );
    // With a gap between them the pads stay apart.
    let mut d = board();
    d.routing.zones.push(zone("left", 10.5, 3.0));
    d.routing.zones.push(zone("right", 16.0, 4.0));
    let a = analyze(&d);
    assert_eq!(a.ratsnest.len(), 1, "{:?}", a.ratsnest);
}
