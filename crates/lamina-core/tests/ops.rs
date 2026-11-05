use lamina_core::model::*;
use lamina_core::*;

fn new_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::create(&dir.path().join("b"), "demo").unwrap();
    (dir, p)
}

fn add_resistor(p: &mut Project, refdes: &str) {
    let mut fp = Footprint::new("R_0603", Provenance::Manual { note: "t".into() });
    for (n, x) in [("1", -0.8), ("2", 0.8)] {
        fp.pads.push(Pad::smd(
            n,
            Point::new(x, 0.0),
            PadShape::Rect {
                width: 0.9,
                height: 0.9,
            },
        ));
    }
    let mut d = p.data.clone();
    d.footprints.insert(fp.id.clone(), fp);
    d.parts.parts.insert(refdes.into(), Part::new("R_0603"));
    p.commit(
        d,
        Touched::file(FileKind::Parts).footprint("R_0603"),
        false,
        "add",
    )
    .unwrap();
}

#[test]
fn outline_and_status() {
    let (_t, mut p) = new_project();
    let r = p
        .set_outline(SetOutline {
            rect: Some(RectSpec {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 30.0,
                corner_radius: 1.0,
            }),
            contour: None,
            cutouts: None,
            strict: false,
        })
        .unwrap();
    assert!(r.fixed.iter().any(|f| f.starts_with("no_outline")), "{r:?}");
    let s = p.status();
    let o = s.outline.unwrap();
    assert!((o.width() - 50.0).abs() < 1e-9);
    // Reopen from disk.
    let root = p.root().to_path_buf();
    drop(p);
    let p2 = Project::open(&root).unwrap();
    assert!(p2.data.board.outline.is_some());
}

#[test]
fn malformed_input_refused() {
    let (_t, mut p) = new_project();
    let e = p.set_outline(SetOutline {
        rect: Some(RectSpec {
            x: 0.0,
            y: 0.0,
            width: -1.0,
            height: 3.0,
            corner_radius: 0.0,
        }),
        contour: None,
        cutouts: None,
        strict: false,
    });
    assert!(e.is_err());
    add_resistor(&mut p, "R1");
    let e = p.connect(Connect {
        net: "GND".into(),
        pads: vec!["R1.7".parse().unwrap()],
        class: None,
        strict: false,
    });
    assert!(e.unwrap_err().to_string().contains("no pad `7`"));
}

#[test]
fn apply_and_report_vs_strict() {
    let (_t, mut p) = new_project();
    add_resistor(&mut p, "R1");
    p.connect(Connect {
        net: "A".into(),
        pads: vec!["R1.1".parse().unwrap()],
        class: None,
        strict: false,
    })
    .unwrap();
    // Same pad into a second net: reported, applied.
    let r = p
        .connect(Connect {
            net: "B".into(),
            pads: vec!["R1.1".parse().unwrap()],
            class: None,
            strict: false,
        })
        .unwrap();
    assert_eq!(r.new_violations[0].rule, "pad_in_two_nets");
    assert!(p.data.nets.nets.contains_key("B"));
    // Strict refuses and leaves the project unchanged.
    let before = p.data.clone();
    let e = p.connect(Connect {
        net: "C".into(),
        pads: vec!["R1.1".parse().unwrap()],
        class: None,
        strict: true,
    });
    assert!(matches!(e, Err(Error::Refused(_))));
    assert_eq!(before, p.data);
    let on_disk = ProjectData::load(p.root()).unwrap();
    assert!(!on_disk.nets.nets.contains_key("C"));
}

#[test]
fn unresolved_pads_are_warnings() {
    let (_t, mut p) = new_project();
    let r = p
        .connect(Connect {
            net: "VCC".into(),
            pads: vec!["U9.1".parse().unwrap()],
            class: None,
            strict: true,
        })
        .unwrap();
    assert!(
        r.new_violations
            .iter()
            .all(|v| v.severity == Severity::Warning)
    );
}

#[test]
fn stackup_refuses_stranding_copper() {
    let (_t, mut p) = new_project();
    p.set_stackup(SetStackup {
        copper_layers: Some(4),
        ..Default::default()
    })
    .unwrap();
    let mut d = p.data.clone();
    d.nets.nets.insert("N".into(), Net::default());
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "N".into(),
        layer: "in2".parse().unwrap(),
        width: 0.2,
        points: vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)],
    });
    p.commit(
        d,
        Touched::file(FileKind::Routing).and(FileKind::Nets),
        true,
        "t",
    )
    .unwrap();
    assert!(
        p.set_stackup(SetStackup {
            copper_layers: Some(2),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        p.set_stackup(SetStackup {
            copper_layers: Some(3),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn lock_blocks_second_open_only_for_live_other_process() {
    let (_t, p) = new_project();
    // Same process may reopen (pid matches).
    let again = Project::open(p.root());
    assert!(again.is_ok());
}

#[test]
fn ids() {
    assert_eq!(next_id("t", ["t1", "t9", "x3", "t10"]), "t11");
    assert_eq!(next_id("v", []), "v1");
}
