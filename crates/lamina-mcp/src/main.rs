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
        Cmd::New { dir, name } => {
            let path = dir.to_string_lossy().to_string();
            if lamina_core::model::ProjectData::is_project(&dir) {
                anyhow::bail!("{path} already contains a project");
            }
            let p = server::open_project(&path, true, name)?;
            println!("{}", serde_json::to_string_pretty(&p.status())?);
            Ok(())
        }
        Cmd::Status { dir } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None)?;
            println!("{}", serde_json::to_string_pretty(&p.status())?);
            Ok(())
        }
        Cmd::Check { dir } => {
            let p = server::open_project(&dir.to_string_lossy(), false, None)?;
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
        Some(dir) => Some(server::open_project(&dir.to_string_lossy(), false, None)?),
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
