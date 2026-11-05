//! MCP tool surface. Each tool locks the open project, runs one core
//! operation and returns its result as JSON text.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lamina_core::model::{self, Severity};
use lamina_core::{
    AddPart, AddParts, Connect, Disconnect, FootprintFromPackage, FootprintFromPads, FootprintId,
    Project, RemovePart, SetNets, SetOutline, SetRules, SetStackup,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const INSTRUCTIONS: &str = "\
Lamina is a PCB layout engine. The board lives in a project folder of JSON files \
(board, parts, nets, placement, routing, notes, cache/footprints). A viewer may be \
watching those files and redraws on every change.

Conventions: millimetres, Y up, rotation in degrees counter-clockwise, fixed origin \
(changing the outline moves nothing else). Layers: copper `top`, `in1`.., `bottom`; \
others `top.silk`, `top.mask`, `top.paste`, `top.courtyard`, `top.fab`, `bottom.*`, \
`outline`. Pads are referenced as `REFDES.PAD`, e.g. `U1.3`.

Every change is applied and its new rule violations are reported. Pass `strict: true` \
to refuse a change that would add an error instead. Malformed input is always refused. \
Start with `open` (create: true for a new project), then `status`.";

pub type Shared = Arc<Mutex<Option<Project>>>;

#[derive(Clone)]
pub struct Lamina {
    project: Shared,
    tool_router: ToolRouter<Lamina>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct OpenParams {
    /// Project folder.
    pub path: String,
    /// Create the project if the folder has none.
    #[serde(default)]
    pub create: bool,
    /// Board name for a new project (defaults to the folder name).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SchemaParams {
    /// One of: board, parts, nets, placement, routing, notes, footprint.
    pub kind: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckParams {
    /// Only report these rule ids.
    #[serde(default)]
    pub rules: Option<Vec<String>>,
    /// Include warnings (default true).
    #[serde(default = "yes")]
    pub warnings: bool,
    /// Maximum number of violations returned (default 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

fn yes() -> bool {
    true
}

pub fn json_result<T: Serialize>(v: &T) -> CallToolResult {
    match serde_json::to_string_pretty(v) {
        Ok(s) => CallToolResult::success(vec![ContentBlock::text(s)]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
    }
}

pub fn error_result(e: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(e.to_string())])
}

impl Lamina {
    pub fn new(project: Option<Project>) -> Self {
        Lamina {
            project: Arc::new(Mutex::new(project)),
            tool_router: Self::tool_router(),
        }
    }

    /// Run `f` on the open project.
    pub fn with<T: Serialize>(
        &self,
        f: impl FnOnce(&mut Project) -> lamina_core::Result<T>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        let Some(p) = guard.as_mut() else {
            return Ok(error_result(
                "no project is open; call `open` with a folder path first",
            ));
        };
        Ok(match f(p) {
            Ok(v) => json_result(&v),
            Err(e) => error_result(e),
        })
    }
}

pub fn open_project(
    path: &str,
    create: bool,
    name: Option<String>,
) -> lamina_core::Result<Project> {
    let dir = PathBuf::from(path);
    if model::ProjectData::is_project(&dir) {
        let mut p = Project::open(&dir)?;
        crate::register_checkers(&mut p);
        return Ok(p);
    }
    if !create {
        return Err(model::Error::NotAProject(dir));
    }
    let name = name.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("board")
            .to_string()
    });
    let mut p = Project::create(&dir, &name)?;
    crate::register_checkers(&mut p);
    Ok(p)
}

#[tool_router]
impl Lamina {
    /// Open a project folder, or create one with `create: true`. Only one
    /// project is open at a time.
    #[tool(annotations(title = "Open project"))]
    async fn open(
        &self,
        Parameters(p): Parameters<OpenParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.project.lock().unwrap_or_else(|p| p.into_inner());
        // Release the previous project's lock first.
        *guard = None;
        match open_project(&p.path, p.create, p.name) {
            Ok(proj) => {
                let s = proj.status();
                *guard = Some(proj);
                Ok(json_result(&s))
            }
            Err(e) => Ok(error_result(e)),
        }
    }

    /// Summary of the open project: size, layers, counts, unplaced parts,
    /// and violation counts per rule.
    #[tool(annotations(title = "Project status", read_only_hint = true))]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        self.with(|p| Ok(p.status()))
    }

    /// Re-read all files from disk, for example after editing JSON by hand.
    #[tool(annotations(title = "Reload from disk"))]
    async fn reload(&self) -> Result<CallToolResult, McpError> {
        self.with(|p| {
            p.reload()?;
            Ok(p.status())
        })
    }

    /// JSON schema of a project file kind.
    #[tool(annotations(title = "File schema", read_only_hint = true))]
    async fn schema(
        &self,
        Parameters(p): Parameters<SchemaParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(match model::schema_for(&p.kind) {
            Some(s) => json_result(&s),
            None => error_result(format!(
                "unknown kind `{}`; use one of {}",
                p.kind,
                model::SCHEMA_KINDS.join(", ")
            )),
        })
    }

    /// List current rule violations (validation and DRC).
    #[tool(annotations(title = "Check", read_only_hint = true))]
    async fn check(
        &self,
        Parameters(p): Parameters<CheckParams>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| {
            let mut v = proj.check();
            if let Some(rules) = &p.rules {
                v.retain(|x| rules.contains(&x.rule));
            }
            if !p.warnings {
                v.retain(|x| x.severity == Severity::Error);
            }
            v.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.rule.cmp(&b.rule)));
            let total = v.len();
            v.truncate(p.limit.unwrap_or(200));
            Ok(serde_json::json!({ "total": total, "violations": v }))
        })
    }

    /// Set the board outline: a rectangle (optionally rounded) or a contour
    /// of lines and arcs. Cut-outs replace existing ones when given.
    #[tool(annotations(title = "Set outline"))]
    async fn set_outline(
        &self,
        Parameters(p): Parameters<SetOutline>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.set_outline(p))
    }

    /// Change copper layer count, thickness, copper weight, finish or colours.
    /// Only the fields given change.
    #[tool(annotations(title = "Set stackup"))]
    async fn set_stackup(
        &self,
        Parameters(p): Parameters<SetStackup>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.set_stackup(p))
    }

    /// Change design rules. Only the given fields change, for example
    /// `{"rules": {"clearance": 0.2, "net_classes": {"power": {"track_width": 0.5}}}}`.
    #[tool(annotations(title = "Set rules"))]
    async fn set_rules(
        &self,
        Parameters(p): Parameters<SetRules>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.set_rules(p))
    }

    /// Set the netlist: `{"nets": {"GND": {"pads": ["U1.1", "C1.2"]}}}`.
    /// Replaces the whole netlist unless `replace` is false.
    #[tool(annotations(title = "Set nets"))]
    async fn set_nets(
        &self,
        Parameters(p): Parameters<SetNets>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.set_nets(p))
    }

    /// Add pads to a net (created if missing), optionally setting its class.
    #[tool(annotations(title = "Connect"))]
    async fn connect(
        &self,
        Parameters(p): Parameters<Connect>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.connect(p))
    }

    /// Remove pads from a net, or the whole net when `pads` is omitted.
    #[tool(annotations(title = "Disconnect"))]
    async fn disconnect(
        &self,
        Parameters(p): Parameters<Disconnect>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.disconnect(p))
    }
    /// Generate an IPC-7351 footprint from a package name. Names: `R_0402`,
    /// `R_0603`, `C_0805`, `LED_0603`, `D_SOD-123`, `SOT-23`, `SOT-23-5`,
    /// `SOT-23-6`, `SOT-223`, `SOIC-8`, `SOIC-16W`, `TSSOP-28`, `SSOP-20`,
    /// `MSOP-10`, `QFN-32_5x5_P0.5_EP3.3x3.3`, `DFN-8_3x3_P0.65`,
    /// `LQFP-48_7x7_P0.5`, `PinHeader_1x03_P2.54`, `BGA-64_8x8_P0.8_5x5`.
    #[tool(annotations(title = "Footprint from package"))]
    async fn footprint_from_package(
        &self,
        Parameters(p): Parameters<FootprintFromPackage>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.footprint_from_package(p))
    }

    /// Create a footprint from pads typed in from a datasheet land pattern.
    /// Courtyard, fab outline and silkscreen are added when no courtyard is given.
    #[tool(annotations(title = "Footprint from pads"))]
    async fn footprint_from_pads(
        &self,
        Parameters(p): Parameters<FootprintFromPads>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.footprint_from_pads(p))
    }

    /// Full footprint JSON: pads, graphics, model, provenance.
    #[tool(annotations(title = "Get footprint", read_only_hint = true))]
    async fn get_footprint(
        &self,
        Parameters(p): Parameters<FootprintId>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.get_footprint(&p.id))
    }

    /// Footprints in the project with pad count, source and users.
    #[tool(annotations(title = "List footprints", read_only_hint = true))]
    async fn list_footprints(&self) -> Result<CallToolResult, McpError> {
        self.with(|proj| Ok(proj.list_footprints()))
    }

    /// Add a part (refdes, footprint id, value, MPN, LCSC number).
    #[tool(annotations(title = "Add part"))]
    async fn add_part(
        &self,
        Parameters(p): Parameters<AddPart>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.add_part(p))
    }

    /// Add several parts at once.
    #[tool(annotations(title = "Add parts"))]
    async fn add_parts(
        &self,
        Parameters(p): Parameters<AddParts>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.add_parts(p))
    }

    /// Remove a part, its placement and (by default) its pads from nets.
    #[tool(annotations(title = "Remove part"))]
    async fn remove_part(
        &self,
        Parameters(p): Parameters<RemovePart>,
    ) -> Result<CallToolResult, McpError> {
        self.with(|proj| proj.remove_part(p))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Lamina {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("lamina", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
