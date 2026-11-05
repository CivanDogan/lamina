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
            if let Some(info) = &im.model {
                if let Some((m, h)) = self.fetch_easyeda_model(info, p.refresh, p.step, &mut notes)
                {
                    im.footprint.model = Some(m);
                    im.footprint.height = h;
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportEasyedaBoard {
    /// Path of the EasyEDA PCB JSON (File > Export > EasyEDA in the standard editor).
    pub path: String,
    /// Fetch the parts' 3D models from EasyEDA (default true).
    #[serde(default = "yes")]
    pub models: bool,
    /// Replace a project that already has a board.
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub strict: bool,
}

impl Project {
    /// Get an EasyEDA 3D model into `cache/models/lcsc/`, STEP when it loads.
    pub(crate) fn fetch_easyeda_model(
        &self,
        info: &lamina_parts::ModelInfo,
        refresh: bool,
        step: bool,
        notes: &mut Vec<String>,
    ) -> Option<(ModelRef, Option<f64>)> {
        let root = self.root().to_path_buf();
        let rel_obj = format!("lcsc/{}.obj", info.uuid);
        let obj_path = root.join(MODEL_DIR).join(&rel_obj);
        let obj = match (!refresh).then(|| read_cached(&obj_path)).flatten() {
            Some(t) => t,
            None => match self.source().model_obj(&info.uuid) {
                Ok(t) => {
                    if let Err(e) = write_atomic(&obj_path, t.as_bytes()) {
                        notes.push(format!("3D model not saved: {e}"));
                        return None;
                    }
                    t
                }
                Err(e) => {
                    notes.push(format!("3D model not downloaded: {e}"));
                    return None;
                }
            },
        };
        let mut model = lamina_parts::model_ref(info, &obj, rel_obj);
        let height = lamina_parts::model_height(info, &obj);
        notes.push(format!("3D model `{}`", info.title));
        if step {
            let rel_step = format!("lcsc/{}.step", info.uuid);
            let step_path = root.join(MODEL_DIR).join(&rel_step);
            if refresh || !step_path.exists() {
                match self.source().model_step(&info.uuid) {
                    Ok(b) => {
                        if let Err(e) = write_atomic(&step_path, &b) {
                            notes.push(format!("STEP not saved: {e}"));
                        }
                    }
                    Err(e) => notes.push(format!("STEP not downloaded: {e}")),
                }
            }
            // The STEP has the same geometry in the same frame with smooth
            // surfaces; use it when it loads.
            if step_path.exists() {
                match lamina_step::load(&step_path) {
                    Ok(_) => model.file = rel_step,
                    Err(e) => notes.push(format!("STEP did not load, using OBJ: {e}")),
                }
            }
        }
        Some((model, height))
    }

    pub fn import_easyeda_board(&mut self, p: ImportEasyedaBoard) -> Result<OpResult> {
        let path = Path::new(&p.path);
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::Json {
            path: path.to_path_buf(),
            source: e,
        })?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("board")
            .to_string();
        let cur = &self.data;
        let has_board = !cur.parts.parts.is_empty()
            || !cur.routing.tracks.is_empty()
            || cur.board.outline.is_some();
        if has_board && !p.replace {
            return Err(Error::Refused(
                "the project already has a board; pass replace: true to overwrite it".into(),
            ));
        }
        let imported = lamina_parts::board::import_board(&doc, &name).map_err(Error::Invalid)?;
        let mut notes = imported.notes;
        let mut d = imported.data;
        d.notes = self.data.notes.clone();
        if p.models {
            let mut found = 0;
            for (fp_id, info) in &imported.models {
                let mut mnotes = Vec::new();
                if let Some((m, h)) = self.fetch_easyeda_model(info, false, true, &mut mnotes) {
                    if let Some(fp) = d.footprints.get_mut(fp_id) {
                        fp.model = Some(m);
                        fp.height = h;
                        found += 1;
                    }
                } else {
                    notes.extend(mnotes);
                }
            }
            notes.push(format!("3D models: {found} of {}", imported.models.len()));
        }
        let mut touched = Touched::default();
        for k in [
            FileKind::Board,
            FileKind::Parts,
            FileKind::Nets,
            FileKind::Placement,
            FileKind::Routing,
        ] {
            touched.files.insert(k);
        }
        for id in d.footprints.keys() {
            touched.footprints.insert(id.clone());
        }
        for id in self.data.footprints.keys() {
            if !d.footprints.contains_key(id) {
                touched.removed_footprints.insert(id.clone());
            }
        }
        let summary = format!(
            "imported {}: {} parts, {} nets, {} tracks, {} vias, {} zones",
            path.display(),
            d.parts.parts.len(),
            d.nets.nets.len(),
            d.routing.tracks.len(),
            d.routing.vias.len(),
            d.routing.zones.len()
        );
        let mut res = self.commit(d, touched, p.strict, summary)?;
        res.notes = notes;
        Ok(res)
    }
}
