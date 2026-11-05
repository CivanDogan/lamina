mod common;

use serde_json::json;

#[test]
fn create_edit_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo");
    let path_s = path.to_string_lossy().to_string();

    let mut c = common::Client::start();
    let names = c.tool_names();
    for t in [
        "open",
        "status",
        "schema",
        "check",
        "set_outline",
        "set_stackup",
        "set_nets",
        "connect",
        "disconnect",
    ] {
        assert!(
            names.contains(&t.to_string()),
            "missing tool {t}: {names:?}"
        );
    }

    let (err, msg) = c.call("status", json!({}));
    assert!(err && msg.as_str().unwrap().contains("no project is open"));

    let s = c.ok("open", json!({"path": path_s, "create": true}));
    assert_eq!(s["name"], "demo");

    let r = c.ok(
        "set_outline",
        json!({"rect": {"width": 40, "height": 25, "corner_radius": 1.5}}),
    );
    assert!(
        r["summary"].as_str().unwrap().contains("40.000 x 25.000"),
        "{r}"
    );

    let (err, msg) = c.call("set_outline", json!({"rect": {"width": 0, "height": 25}}));
    assert!(err, "{msg}");

    c.ok(
        "set_stackup",
        json!({"copper_layers": 4, "mask_color": "black"}),
    );
    c.ok(
        "set_nets",
        json!({"nets": {"GND": {"pads": ["U1.1", "C1.2"]}, "3V3": {"pads": ["U1.2"]}}}),
    );
    let r = c.ok("connect", json!({"net": "GND", "pads": ["C2.2"]}));
    assert_eq!(r["summary"], "net GND now has 3 pad(s)");

    let chk = c.ok("check", json!({"warnings": true}));
    assert!(chk["total"].as_u64().unwrap() >= 4, "{chk}");

    let sch = c.ok("schema", json!({"kind": "routing"}));
    assert!(sch.to_string().contains("tracks"));

    drop(c);

    // Files are on disk and readable by a fresh process.
    let nets = std::fs::read_to_string(path.join("nets.json")).unwrap();
    assert!(nets.contains("\"C2.2\""));
    let board = std::fs::read_to_string(path.join("board.json")).unwrap();
    assert!(board.contains("\"copper_layers\": 4"), "{board}");

    let mut c = common::Client::start();
    let s = c.ok("open", json!({"path": path_s}));
    assert_eq!(s["copper_layers"], 4);
    assert_eq!(s["nets"], 2);
}
