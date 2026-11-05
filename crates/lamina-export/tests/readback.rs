//! Export a board, read the Gerbers back and compare copper area with the model.

use geo::{Area, BooleanOps, MultiPolygon};
use lamina_footprint::{Options, from_package};
use lamina_geom::fill::inflate;
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("rb");
    d.board.outline = Some(Contour::rounded_rect(0.0, 0.0, 30.0, 20.0, 1.0));
    for pkg in ["R_0603", "SOIC-8", "PinHeader_1x03_P2.54"] {
        let fp = from_package(pkg, &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp);
    }
    for (r, f, x, y, rot) in [
        ("R1", "R_0603", 5.0, 10.0, 30.0),
        ("U1", "SOIC-8", 15.0, 10.0, 90.0),
        ("J1", "PinHeader_1x03_P2.54", 25.0, 10.0, 0.0),
    ] {
        d.parts.parts.insert(r.into(), Part::new(f));
        let mut pl = Placement::at(x, y);
        pl.rotation = rot;
        d.placement.placements.insert(r.into(), pl);
    }
    d.nets.nets.insert(
        "GND".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap(), "J1.1".parse().unwrap()],
            class: None,
        },
    );
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "GND".into(),
        layer: Layer::TOP,
        width: 0.3,
        points: vec![
            Point::new(2.0, 2.0),
            Point::new(10.0, 2.0),
            Point::new(14.0, 5.0),
        ],
    });
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("GND".into()),
        layers: vec![Layer::BOTTOM],
        outline: Contour::rect(0.0, 0.0, 30.0, 20.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    d
}

fn model_copper(a: &lamina_drc::Analysis, layer: u8) -> MultiPolygon<f64> {
    let mut acc = MultiPolygon::new(vec![]);
    for it in a.geometry.copper.iter().filter(|i| i.layer == layer) {
        acc = acc.union(&inflate(&it.prim, 0.0));
    }
    for f in &a.fills {
        if f.layer.copper().map(|c| c.index(2)) == Some(layer) {
            for p in &f.polygons {
                acc = acc.union(&inflate(&p.prim(), 0.0));
            }
        }
    }
    acc
}

#[test]
fn copper_round_trip() {
    let d = board();
    let a = lamina_drc::analyze(&d);
    let s = lamina_scene::build_2d(&d, &a);
    let files = lamina_export::gerbers(&s, "rb");
    for (layer, ext) in [(0u8, "GTL"), (1, "GBL")] {
        let f = files.iter().find(|f| f.name.ends_with(ext)).unwrap();
        let text = String::from_utf8(f.contents.clone()).unwrap();
        assert!(text.contains("%TF.FileFunction,Copper,"));
        let got = lamina_export::reader::read_gerber(&text).unwrap();
        let want = model_copper(&a, layer);
        let xor = got.xor(&want).unsigned_area();
        let area = want.unsigned_area();
        assert!(area > 1.0, "{ext}: model copper area {area}");
        assert!(xor / area < 0.01, "{ext}: xor {xor} vs area {area}");
    }
}

#[test]
fn drills_round_trip() {
    let d = board();
    let a = lamina_drc::analyze(&d);
    let files = lamina_export::drills(&a.geometry, "rb");
    let pth = files
        .iter()
        .find(|f| f.name.ends_with("PTH.DRL") && !f.name.contains("NPTH"))
        .unwrap();
    let holes =
        lamina_export::reader::read_excellon(&String::from_utf8(pth.contents.clone()).unwrap())
            .unwrap();
    assert_eq!(holes.len(), 3);
    for (dia, p, _) in &holes {
        assert!((dia - 1.0).abs() < 1e-6);
        assert!((p.x - 25.0).abs() < 1e-3);
    }
}

#[test]
fn bom_and_cpl() {
    let mut d = board();
    d.parts.parts.get_mut("R1").unwrap().lcsc = Some("C25804".into());
    d.parts.parts.get_mut("R1").unwrap().value = Some("10k".into());
    let (out, warnings) = lamina_export::assembly(&d, "rb");
    let bom = String::from_utf8(out[0].contents.clone()).unwrap();
    assert!(bom.contains("10k,R1,R_0603,C25804"), "{bom}");
    let cpl = String::from_utf8(out[1].contents.clone()).unwrap();
    assert!(cpl.contains("U1,15.0000mm,10.0000mm,Top,90"), "{cpl}");
    assert!(warnings.iter().any(|w| w.contains("U1 has no LCSC")));
}
