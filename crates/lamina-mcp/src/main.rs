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
    },
    /// Import an EasyEDA PCB JSON into a project (created when missing).
    ImportEasyeda {
        file: PathBuf,
        dir: PathBuf,
        /// Skip fetching 3D models from EasyEDA.
        #[arg(long)]
        no_models: bool,
    },
    /// Import Gerber and drill files (folder or zip) into a project as artwork.
    ImportGerbers { path: PathBuf, dir: PathBuf },
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
        Cmd::ImportKicad { file, dir, online } => {
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
                replace: false,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ImportEasyeda {
            file,
            dir,
            no_models,
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
                replace: false,
                strict: false,
            })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Cmd::ImportGerbers { path, dir } => {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string);
            let mut p = server::open_project(&dir.to_string_lossy(), true, name, false)?;
            let r = p.import_gerbers(lamina_core::ImportGerbers {
                path: path.to_string_lossy().to_string(),
                replace: false,
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
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
            println!("{}", serde_json::to_string_pretty(&p.status())?);
            Ok(())
        }
        Cmd::Check { dir } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None, false)?;
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
