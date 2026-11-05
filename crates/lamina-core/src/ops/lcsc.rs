use std::path::Path;

use lamina_model::error::Result;
use lamina_model::*;
use lamina_parts::PartHit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SearchPart {
    /// Keywords, MPN or LCSC number, e.g. `AMS1117-3.3`, `0603 10k`, `C6186`.
    pub query: String,
    /// Maximum results (default 10).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only JLCPCB basic parts.
    #[serde(default)]
    pub basic_only: bool,
    /// Drop parts with no stock (default true).
    #[serde(default = "yes")]
    pub in_stock: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportLcsc {
    /// LCSC number, e.g. `C6186`.
    pub lcsc: String,
    /// Also add a part with this refdes.
    #[serde(default)]
    pub refdes: Option<String>,
    /// Part value (defaults to the MPN).
    #[serde(default)]
    pub value: Option<String>,
    /// Footprint id to use instead of the EasyEDA package name.
    #[serde(default)]
    pub footprint_id: Option<String>,
    /// Download again instead of using the project cache.
    #[serde(default)]
    pub refresh: bool,
    /// Also download the STEP model (default true).
    #[serde(default = "yes")]
    pub step: bool,
    /// Replace an existing part with the same refdes.
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportMany {
    pub parts: Vec<ImportLcsc>,
}

/// Today's date as YYYY-MM-DD (UTC).
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn read_cached(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

impl Project {
    pub fn search_part(&self, p: SearchPart) -> Result<Vec<PartHit>> {
        if p.query.trim().is_empty() {
            return Err(Error::invalid("query is empty"));
        }
        let limit = p.limit.unwrap_or(10).clamp(1, 50);
        let mut hits = self
            .source()
            .search(p.query.trim(), limit * 2)
            .map_err(Error::Invalid)?;
        if p.basic_only {
            hits.retain(|h| h.basic);
        }
        if p.in_stock {
            hits.retain(|h| h.stock > 0);
        }
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn import_lcsc(&mut self, p: ImportLcsc) -> Result<OpResult> {
        let lcsc = p.lcsc.trim().to_ascii_uppercase();
        let root = self.root().to_path_buf();
        let cache = root.join(PARTS_DIR).join(format!("{lcsc}.json"));
        let raw: serde_json::Value = match (!p.refresh).then(|| read_cached(&cache)).flatten() {
            Some(t) => serde_json::from_str(&t)
                .map_err(|e| Error::invalid(format!("{}: {e}", cache.display())))?,
            None => {
                let v = self.source().component(&lcsc).map_err(Error::Invalid)?;
                let text = serde_json::to_string(&v).map_err(|e| Error::invalid(e.to_string()))?;
                write_atomic(&cache, text.as_bytes())?;
                v
            }
        };
        let mut im = lamina_parts::convert(&raw, &lcsc, &today()).map_err(Error::Invalid)?;
        if let Some(id) = &p.footprint_id {
            im.footprint.id = id.clone();
        }
        let mut notes = im.notes.clone();
        let mut d = self.data.clone();
        let mut touched = Touched::default();
        let fp_id = im.footprint.id.clone();

        let reuse = !p.refresh && d.footprints.contains_key(&fp_id);
        if reuse {
            notes.push(format!(
                "footprint `{fp_id}` already in the project, kept as is"
            ));
        } else {
            // 3D model.
            if let Some(info) = &im.model {
                let rel_obj = format!("lcsc/{}.obj", info.uuid);
                let obj_path = root.join(MODEL_DIR).join(&rel_obj);
                let obj = match (!p.refresh).then(|| read_cached(&obj_path)).flatten() {
                    Some(t) => Some(t),
                    None => match self.source().model_obj(&info.uuid) {
                        Ok(t) => {
                            write_atomic(&obj_path, t.as_bytes())?;
                            Some(t)
                        }
                        Err(e) => {
                            notes.push(format!("3D model not downloaded: {e}"));
                            None
                        }
                    },
                };
                if let Some(obj) = obj {
                    im.footprint.model = Some(lamina_parts::model_ref(info, &obj, rel_obj));
                    im.footprint.height = lamina_parts::model_height(info, &obj);
                    notes.push(format!("3D model `{}`", info.title));
                }
                if p.step {
                    let rel_step = format!("lcsc/{}.step", info.uuid);
                    let step_path = root.join(MODEL_DIR).join(&rel_step);
                    if p.refresh || !step_path.exists() {
                        match self.source().model_step(&info.uuid) {
                            Ok(b) => write_atomic(&step_path, &b)?,
                            Err(e) => notes.push(format!("STEP not downloaded: {e}")),
                        }
                    }
                    // The STEP has the same geometry in the same frame with
                    // smooth surfaces; use it when it loads.
                    if step_path.exists() {
                        match lamina_step::load(&step_path) {
                            Ok(_) => {
                                if let Some(m) = im.footprint.model.as_mut() {
                                    m.file = rel_step;
                                }
                            }
                            Err(e) => notes.push(format!("STEP did not load, using OBJ: {e}")),
                        }
                    }
                }
            } else {
                notes.push("EasyEDA has no 3D model for this footprint".into());
            }
            touched = touched.footprint(fp_id.clone());
            d.footprints.insert(fp_id.clone(), im.footprint.clone());
        }

        let fp = &d.footprints[&fp_id];
        let pads: Vec<&str> = {
            let mut v: Vec<&str> = fp
                .pads
                .iter()
                .map(|x| x.number.as_str())
                .filter(|n| !n.is_empty())
                .collect();
            v.sort_by(|a, b| natural(a, b));
            v.dedup();
            v
        };
        let meta = &im.meta;
        let class = match meta.basic {
            Some(true) => "basic",
            Some(false) => "extended",
            None => "unknown class",
        };
        let mut summary = format!(
            "{lcsc} {} ({}), {class}: footprint `{fp_id}` with {} pads [{}]",
            meta.mpn.as_deref().unwrap_or("?"),
            meta.package.as_deref().unwrap_or("?"),
            fp.pads.len(),
            pads.join(" ")
        );

        let parts_generated = lamina_build::owned(self.root()).contains(&FileKind::Parts);
        if let (Some(r), true) = (p.refdes.as_deref(), parts_generated) {
            let q = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
            let mut fields = vec![format!("footprint: '{}'", q(&fp_id))];
            if let Some(v) = p.value.as_deref().or(meta.mpn.as_deref()) {
                fields.push(format!("value: '{}'", q(v)));
            }
            if let Some(m) = &meta.mpn {
                fields.push(format!("mpn: '{}'", q(m)));
            }
            if let Some(m) = &meta.manufacturer {
                fields.push(format!("manufacturer: '{}'", q(m)));
            }
            fields.push(format!("lcsc: '{lcsc}'"));
            notes.push(format!(
                "parts.json is generated from {}; add the part there: {r}: {{ {} }}",
                lamina_build::SOURCE_FILE,
                fields.join(", ")
            ));
            summary = format!("{r} not added (see notes): {summary}");
        } else if let Some(r) = p.refdes {
            if !valid_refdes(&r) {
                return Err(Error::invalid(format!("bad refdes `{r}`")));
            }
            if d.parts.parts.contains_key(&r) && !p.replace {
                return Err(Error::invalid(format!(
                    "{r} exists; pass replace: true to change it"
                )));
            }
            let mut part = Part::new(fp_id.clone());
            part.value = p.value.or_else(|| meta.mpn.clone());
            part.mpn = meta.mpn.clone();
            part.manufacturer = meta.manufacturer.clone();
            part.lcsc = Some(lcsc.clone());
            part.description = meta.description.clone();
            d.parts.parts.insert(r.clone(), part);
            touched = touched.and(FileKind::Parts);
            summary = format!("{r}: {summary}");
        }
        let mut res = self.commit(d, touched, p.strict, summary)?;
        res.notes = notes;
        Ok(res)
    }

    /// Import several LCSC parts; stops at the first error.
    pub fn import_many(&mut self, p: ImportMany) -> Result<Vec<OpResult>> {
        let mut out = Vec::new();
        for item in p.parts {
            let lcsc = item.lcsc.clone();
            out.push(
                self.import_lcsc(item)
                    .map_err(|e| Error::invalid(format!("{lcsc}: {e}")))?,
            );
        }
        Ok(out)
    }
}

/// Compare pad numbers so that 2 < 10 and A2 < A10.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn split(s: &str) -> (String, u64) {
        let p = s.trim_end_matches(|c: char| c.is_ascii_digit());
        (p.to_string(), s[p.len()..].parse().unwrap_or(0))
    }
    let (pa, na) = split(a);
    let (pb, nb) = split(b);
    pa.cmp(&pb).then(na.cmp(&nb)).then(a.cmp(b))
}
