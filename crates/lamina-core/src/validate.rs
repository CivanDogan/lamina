//! Model-level checks that run after every change. DRC adds geometric
//! checks on top of these.

use std::collections::BTreeMap;

use lamina_model::*;

pub fn validate(d: &ProjectData) -> Vec<Violation> {
    let mut v = Vec::new();
    let layers = d.board.stackup.copper_layers;

    match &d.board.outline {
        None => v.push(Violation::warning("no_outline", "board outline is not set")),
        Some(c) => {
            if let Err(e) = c.check() {
                v.push(Violation::error("bad_outline", e));
            }
        }
    }

    for (r, p) in &d.parts.parts {
        if !d.footprints.contains_key(&p.footprint) {
            v.push(
                Violation::error(
                    "missing_footprint",
                    format!(
                        "{r} uses footprint `{}`, which is not in cache/footprints",
                        p.footprint
                    ),
                )
                .items([r.clone()]),
            );
        }
    }

    for r in d.placement.placements.keys() {
        if !d.parts.parts.contains_key(r) {
            v.push(
                Violation::error(
                    "placement_unknown_part",
                    format!("placement for unknown part {r}"),
                )
                .items([r.clone()]),
            );
        }
    }

    let mut seen: BTreeMap<&PadRef, &str> = BTreeMap::new();
    for (name, net) in &d.nets.nets {
        if let Some(c) = &net.class {
            if !d.board.rules.net_classes.contains_key(c) {
                v.push(
                    Violation::warning(
                        "unknown_net_class",
                        format!("net {name} uses undefined class `{c}`"),
                    )
                    .items([name.clone()]),
                );
            }
        }
        for p in &net.pads {
            if let Some(other) = seen.insert(p, name) {
                if other != name {
                    v.push(
                        Violation::error(
                            "pad_in_two_nets",
                            format!("{p} is in nets {other} and {name}"),
                        )
                        .items([
                            p.to_string(),
                            other.to_string(),
                            name.clone(),
                        ]),
                    );
                }
            }
            match d.parts.parts.get(&p.part) {
                None => v.push(
                    Violation::warning(
                        "unresolved_pad",
                        format!("{p}: part {} does not exist yet", p.part),
                    )
                    .items([p.to_string()]),
                ),
                Some(part) => {
                    if let Some(fp) = d.footprints.get(&part.footprint) {
                        if fp.pad(&p.pad).is_none() {
                            v.push(
                                Violation::error(
                                    "unknown_pad",
                                    format!("{p}: footprint `{}` has no pad `{}`", fp.id, p.pad),
                                )
                                .items([p.to_string()]),
                            );
                        }
                    }
                }
            }
        }
    }

    let net_exists = |n: &str| d.nets.nets.contains_key(n);
    for t in &d.routing.tracks {
        if !t.layer.is_copper() || !t.layer.exists_in(layers) {
            v.push(
                Violation::error(
                    "layer_not_in_stackup",
                    format!("track {} is on {}", t.id, t.layer),
                )
                .items([t.id.clone()]),
            );
        }
        if !net_exists(&t.net) {
            v.push(
                Violation::error(
                    "unknown_net",
                    format!("track {} uses unknown net {}", t.id, t.net),
                )
                .items([t.id.clone()]),
            );
        }
    }
    for via in &d.routing.vias {
        for l in [via.from, via.to].into_iter().flatten() {
            if !l.is_copper() || !l.exists_in(layers) {
                v.push(
                    Violation::error("layer_not_in_stackup", format!("via {} spans {l}", via.id))
                        .items([via.id.clone()]),
                );
            }
        }
        if !net_exists(&via.net) {
            v.push(
                Violation::error(
                    "unknown_net",
                    format!("via {} uses unknown net {}", via.id, via.net),
                )
                .items([via.id.clone()]),
            );
        }
    }
    for z in &d.routing.zones {
        for l in &z.layers {
            if !l.is_copper() || !l.exists_in(layers) {
                v.push(
                    Violation::error("layer_not_in_stackup", format!("zone {} is on {l}", z.id))
                        .items([z.id.clone()]),
                );
            }
        }
        if let Some(n) = &z.net {
            if !net_exists(n) {
                v.push(
                    Violation::error("unknown_net", format!("zone {} uses unknown net {n}", z.id))
                        .items([z.id.clone()]),
                );
            }
        }
    }
    v
}
