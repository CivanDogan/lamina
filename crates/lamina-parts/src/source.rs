//! The sources of part data. The public endpoints are not official, so all
//! access goes through [`PartSource`] and a failure is a plain error.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PartHit {
    /// The LCSC number, for example `C6186`.
    pub lcsc: String,
    pub mpn: String,
    pub manufacturer: Option<String>,
    pub package: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub stock: u64,
    /// The unit price in USD at quantity 1, if it is known.
    pub price: Option<f64>,
    /// True for a JLCPCB basic part (no extended-part fee).
    pub basic: bool,
    pub datasheet: Option<String>,
}

pub trait PartSource: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<PartHit>, String>;
    /// Gets the EasyEDA component record for an LCSC number.
    fn component(&self, lcsc: &str) -> Result<Value, String>;
    /// Gets the EasyEDA OBJ mesh for a model uuid.
    fn model_obj(&self, uuid: &str) -> Result<String, String>;
    /// Gets the STEP model for a model uuid.
    fn model_step(&self, uuid: &str) -> Result<Vec<u8>, String>;
}

const UA: &str = concat!("lamina/", env!("CARGO_PKG_VERSION"));
const SEARCH_URL: &str =
    "https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList";
const COMPONENT_URL: &str = "https://easyeda.com/api/products/{}/components?version=6.4.19.5";
const OBJ_URL: &str = "https://modules.easyeda.com/3dmodel/{}";
const STEP_URL: &str = "https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{}";

pub struct HttpSource {
    agent: ureq::Agent,
}

impl Default for HttpSource {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(40)))
            .build();
        HttpSource {
            agent: config.into(),
        }
    }
}

fn valid_lcsc(s: &str) -> bool {
    s.len() >= 2
        && s.len() <= 16
        && s.starts_with('C')
        && s[1..].chars().all(|c| c.is_ascii_digit())
}

fn valid_uuid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

impl HttpSource {
    fn get_text(&self, url: &str) -> Result<String, String> {
        let mut resp = self
            .agent
            .get(url)
            .header("User-Agent", UA)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("GET {url}: {e}"))
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut resp = self
            .agent
            .get(url)
            .header("User-Agent", UA)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(128 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("GET {url}: {e}"))
    }
}

impl PartSource for HttpSource {
    fn search(&self, query: &str, limit: usize) -> Result<Vec<PartHit>, String> {
        let body = json!({"keyword": query, "currentPage": 1, "pageSize": limit.clamp(1, 50)});
        let mut resp = self
            .agent
            .post(SEARCH_URL)
            .header("User-Agent", UA)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("part search: {e}"))?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("part search: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("part search: {e}"))?;
        parse_search(&v)
    }

    fn component(&self, lcsc: &str) -> Result<Value, String> {
        if !valid_lcsc(lcsc) {
            return Err(format!(
                "`{lcsc}` is not an LCSC number. Use a number like C6186."
            ));
        }
        let text = self.get_text(&COMPONENT_URL.replace("{}", lcsc))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{lcsc}: {e}"))?;
        if v.get("success").and_then(|s| s.as_bool()) == Some(false)
            || v.get("result").is_none_or(|r| r.is_null())
        {
            return Err(format!("{lcsc}: EasyEDA has no component for this number"));
        }
        Ok(v)
    }

    fn model_obj(&self, uuid: &str) -> Result<String, String> {
        if !valid_uuid(uuid) {
            return Err(format!("the model uuid `{uuid}` is not valid"));
        }
        let t = self.get_text(&OBJ_URL.replace("{}", uuid))?;
        if !t.lines().any(|l| l.starts_with("v ")) {
            return Err(format!("model {uuid}: the response is not an OBJ mesh"));
        }
        Ok(t)
    }

    fn model_step(&self, uuid: &str) -> Result<Vec<u8>, String> {
        if !valid_uuid(uuid) {
            return Err(format!("the model uuid `{uuid}` is not valid"));
        }
        let b = self.get_bytes(&STEP_URL.replace("{}", uuid))?;
        if !b.starts_with(b"ISO-10303-21") {
            return Err(format!("model {uuid}: the response is not a STEP file"));
        }
        Ok(b)
    }
}

pub fn parse_search(v: &Value) -> Result<Vec<PartHit>, String> {
    let list = v
        .pointer("/data/componentPageInfo/list")
        .ok_or("the search response has an unknown format")?;
    let Some(list) = list.as_array() else {
        return Ok(Vec::new());
    };
    let s = |x: &Value, k: &str| {
        x.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    Ok(list
        .iter()
        .filter_map(|x| {
            let lcsc = s(x, "componentCode")?;
            Some(PartHit {
                mpn: s(x, "componentModelEn").unwrap_or_else(|| lcsc.clone()),
                lcsc,
                manufacturer: s(x, "componentBrandEn"),
                package: s(x, "componentSpecificationEn"),
                description: s(x, "describe"),
                category: s(x, "componentTypeEn"),
                stock: x.get("stockCount").and_then(|v| v.as_u64()).unwrap_or(0),
                price: x
                    .get("componentPrices")
                    .and_then(|p| p.as_array())
                    .and_then(|p| p.first())
                    .and_then(|p| p.get("productPrice"))
                    .and_then(|p| p.as_f64()),
                basic: s(x, "componentLibraryType").as_deref() == Some("base"),
                datasheet: s(x, "dataManualUrl").or_else(|| s(x, "lcscGoodsUrl")),
            })
        })
        .collect())
}

/// Reads recorded responses from a folder: `search-<query>.json`,
/// `<lcsc>.json`, `<uuid>.obj`, `<uuid>.step`. Tests and offline work use it.
pub struct DirSource {
    pub dir: PathBuf,
}

impl PartSource for DirSource {
    fn search(&self, query: &str, _limit: usize) -> Result<Vec<PartHit>, String> {
        let name: String = query
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let p = self.dir.join(format!("search-{name}.json"));
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        parse_search(&serde_json::from_str(&t).map_err(|e| e.to_string())?)
    }

    fn component(&self, lcsc: &str) -> Result<Value, String> {
        let p = self.dir.join(format!("{lcsc}.json"));
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    }

    fn model_obj(&self, uuid: &str) -> Result<String, String> {
        let p = self.dir.join(format!("{uuid}.obj"));
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
    }

    fn model_step(&self, uuid: &str) -> Result<Vec<u8>, String> {
        let p = self.dir.join(format!("{uuid}.step"));
        std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
    }
}
