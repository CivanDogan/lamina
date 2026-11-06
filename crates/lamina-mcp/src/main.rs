mod server;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use lamina_core::Project;
use rmcp::ServiceExt;

#[derive(Parser)]
#[command(
    name = "lamina",
    version,
    about = "PCB layout engine for coding agents"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the MCP server on stdio.
    Mcp {
        /// Open this project at startup.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Create a new project.
    New {
        dir: PathBuf,
        #[arg(long)]
        name: Option<String>,
        /// Start from a modular design.jsonnet source.
        #[arg(long)]
        modular: bool,
    },
    /// Build a board's design.jsonnet into its JSON files.
    Build { dir: PathBuf },
    /// Import a KiCad board file into a project (created when missing).
    ImportKicad {
        file: PathBuf,
        dir: PathBuf,
        /// Fetch missing 3D models from the official KiCad libraries.
        #[arg(long)]
        online: bool,
        /// Overwrite a board already in the project.
        #[arg(long)]
        replace: bool,
    },
    /// Import an EasyEDA PCB JSON into a project (created when missing).
    ImportEasyeda {
        file: PathBuf,
        dir: PathBuf,
        /// Skip fetching 3D models from EasyEDA.
        #[arg(long)]
        no_models: bool,
        /// Overwrite a board already in the project.
        #[arg(long)]
        replace: bool,
    },
    /// Import Gerber and drill files (folder or zip) into a project as artwork.
    ImportGerbers {
        path: PathBuf,
        dir: PathBuf,
        /// Overwrite a board already in the project.
        #[arg(long)]
        replace: bool,
    },
    /// Write the assembled board as a STEP file.
    ExportStep {
        dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Drill vias through the board too.
        #[arg(long)]
        vias: bool,
        /// Leave the part models out.
        #[arg(long)]
        board_only: bool,
    },
    /// Write a Specctra DSN for an external autorouter.
    ExportDsn {
        dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Leave existing routes out.
        #[arg(long)]
        reroute_all: bool,
    },
    /// Read a Specctra session (.ses) back into a project.
    ImportSes { file: PathBuf, dir: PathBuf },
    /// Write a project as a .kicad_pcb file.
    ExportKicad {
        dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Copy one block's layout onto others. A block is a group name or a
    /// comma-separated refdes list (`D1,R1`).
    RepeatLayout {
        dir: PathBuf,
        #[arg(long)]
        from: String,
        #[arg(long, required = true)]
        to: Vec<String>,
        /// Part that stays where it is (in the source or a target).
        #[arg(long)]
        anchor: Option<String>,
        /// Copy placement only.
        #[arg(long)]
        no_routing: bool,
        /// Also move locked parts.
        #[arg(long)]
        force: bool,
    },
    /// Save a laid-out block as layouts/<name>.json (workspace library, else board).
    SaveLayout {
        dir: PathBuf,
        #[arg(long)]
        from: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        anchor: Option<String>,
        #[arg(long)]
        replace: bool,
    },
    /// Put a saved layout onto blocks of a board.
    PlaceLayout {
        dir: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, required = true)]
        to: Vec<String>,
        /// Target part that stays where it is.
        #[arg(long)]
        anchor: Option<String>,
        #[arg(long)]
        no_routing: bool,
        #[arg(long)]
        force: bool,
    },
    /// Create a workspace (lamina.json with a lib folder) in a folder.
    Workspace {
        dir: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Print project status as JSON.
    Status { dir: PathBuf },
    /// List violations as JSON.
    Check { dir: PathBuf },
    /// Print the JSON schema of a file kind.
    Schema { kind: String },
}

/// A block on the command line: `name` is a group, `A,B,C` a part list.
fn block(s: &str) -> lamina_core::reuse::Block {
    if s.contains(',') {
        lamina_core::reuse::Block::Parts(
            s.split(',')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect(),
        )
    } else {
        lamina_core::reuse::Block::Group(s.to_string())
    }
}

/// Hook for checkers from other crates (DRC). Called whenever a project opens.
pub fn register_checkers(_p: &mut Project) {}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Mcp { project } => run_mcp(project),
        Cmd::New { dir, name, modular } => {
            let path = dir.to_string_lossy().to_string();
            if lamina_core::model::ProjectData::is_project(&dir) {
                anyhow::bail!("{path} already contains a project");
            }
            let p = server::open_project(&path, true, name, modular)?;
            if let Some(mut ws) = lamina_core::model::Workspace::find(&dir) {
                ws.add_board(&dir)?;
            }
            println!("{}", serde_json::to_string_pretty(&p.status())?);
            Ok(())
        }
        Cmd::ImportKicad {
            file,
            dir,
            online,
            replace,
        } => {
            let path = dir.to_string_lossy().to_string();
            let name = file
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            let mut p = server::open_project(&path, true, name, false)?;
            let r = p.import_kicad_board(lamina_core::ImportKicadBoard {
                path: file.to_string_lossy().to_string(),
                models: true,
                online,
                replace,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ImportEasyeda {
            file,
            dir,
            no_models,
            replace,
        } => {
            let path = dir.to_string_lossy().to_string();
            let name = file
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            let mut p = server::open_project(&path, true, name, false)?;
            let r = p.import_easyeda_board(lamina_core::ImportEasyedaBoard {
                path: file.to_string_lossy().to_string(),
                models: !no_models,
                replace,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ExportStep {
            dir,
            out,
            vias,
            board_only,
        } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.export_step(lamina_core::ExportStep {
                path: out.map(|o| o.to_string_lossy().to_string()),
                vias,
                board_only,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ImportGerbers { path, dir, replace } => {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            let mut p = server::open_project(&dir.to_string_lossy(), true, name, false)?;
            let r = p.import_gerbers(lamina_core::ImportGerbers {
                path: path.to_string_lossy().to_string(),
                replace,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ExportDsn {
            dir,
            out,
            reroute_all,
        } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.export_specctra(lamina_core::ExportSpecctra {
                path: out.map(|o| o.to_string_lossy().to_string()),
                reroute_all,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ImportSes { file, dir } => {
            let mut p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.import_specctra_session(lamina_core::ImportSpecctraSession {
                path: file.to_string_lossy().to_string(),
                replace: true,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ExportKicad { dir, out } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.export_kicad(lamina_core::ExportKicad {
                path: out.map(|o| o.to_string_lossy().to_string()),
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::RepeatLayout {
            dir,
            from,
            to,
            anchor,
            no_routing,
            force,
        } => {
            let mut p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.repeat_layout(lamina_core::RepeatLayout {
                from: block(&from),
                to: to.iter().map(|t| block(t)).collect(),
                anchor,
                at: None,
                routing: !no_routing,
                force,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::SaveLayout {
            dir,
            from,
            name,
            anchor,
            replace,
        } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.save_layout_block(lamina_core::SaveLayoutBlock {
                from: block(&from),
                name,
                anchor,
                replace,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::PlaceLayout {
            dir,
            name,
            to,
            anchor,
            no_routing,
            force,
        } => {
            let mut p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            let r = p.place_layout_block(lamina_core::PlaceLayoutBlock {
                name,
                to: to.iter().map(|t| block(t)).collect(),
                anchor,
                at: None,
                routing: !no_routing,
                force,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::Build { dir } => {
            let mut p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            println!("{}", serde_json::to_string_pretty(&p.build()?)?);
            Ok(())
        }
        Cmd::Workspace { dir, name } => {
            let name = name.unwrap_or_else(|| {
                dir.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("workspace")
                    .to_string()
            });
            let ws = lamina_core::model::Workspace::create(&dir, &name)?;
            println!("{}", serde_json::to_string_pretty(&ws.file)?);
            Ok(())
        }
        Cmd::Status { dir } => {
            let p = server::read_project(&dir.to_string_lossy())?;
            println!("{}", serde_json::to_string_pretty(&p.status())?);
            Ok(())
        }
        Cmd::Check { dir } => {
            let p = server::read_project(&dir.to_string_lossy())?;
            println!("{}", serde_json::to_string_pretty(&p.check())?);
            Ok(())
        }
        Cmd::Schema { kind } => {
            let s = lamina_core::model::schema_for(&kind)
                .with_context(|| format!("unknown kind `{kind}`"))?;
            println!("{}", serde_json::to_string_pretty(&s)?);
            Ok(())
        }
    }
}

fn run_mcp(project: Option<PathBuf>) -> anyhow::Result<()> {
    let initial = match project {
        Some(dir) => Some(server::open_project(
            &dir.to_string_lossy(),
            false,
            None,
            false,
        )?),
        None => None,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let service = server::Lamina::new(initial)
            .serve(rmcp::transport::stdio())
            .await
            .context("starting MCP server")?;
        service.waiting().await?;
        Ok(())
    })
}
