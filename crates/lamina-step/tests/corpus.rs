//! Vendor STEP files must load and match the EasyEDA OBJ export of the same model.

#[test]
fn corpus_matches_obj() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "step") {
            continue;
        }
        n += 1;
        let m = lamina_step::step::load(&std::fs::read(&p).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let o = lamina_step::obj::parse(&std::fs::read_to_string(p.with_extension("obj")).unwrap())
            .unwrap();
        let (a, b) = (m.bounds().unwrap(), o.bounds().unwrap());
        for k in 0..3 {
            assert!(
                (a.0[k] - b.0[k]).abs() < 0.02,
                "{}: min {:?} vs {:?}",
                p.display(),
                a.0,
                b.0
            );
            assert!(
                (a.1[k] - b.1[k]).abs() < 0.02,
                "{}: max {:?} vs {:?}",
                p.display(),
                a.1,
                b.1
            );
        }
        assert!(m.triangles() > 100);
    }
    assert!(n >= 3);
}
