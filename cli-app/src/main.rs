mod permissions;
mod ui;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::{Parser, Subcommand};
use include_dir::{Dir, include_dir};
use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager, har_share::HarShareService, http::start_http_server_with_routes,
    session_share::SessionShareService, skill_install,
};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::{BufferLayer, LogBuffer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::permissions::{CliPermissionService, UiAccess};

/// The ProxyCrab agent skill, embedded at compile time. Mirrors the desktop
/// app's bundled resources: SKILL.md, references/, and scripts/ (no evals/).
static SKILL_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../skills/proxycrab");

/// Run the ProxyCrab proxy headless, without the desktop GUI.
#[derive(Parser)]
#[command(name = "proxycrab-cli", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// With no subcommand, the proxy runs with these arguments.
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Run the proxy headless (default when no subcommand is given).
    Run(RunArgs),
    /// Install the bundled ProxyCrab agent skill.
    InstallSkill(InstallSkillArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    /// Workspace directory used as the data root (sessions, scripts, CA,
    /// config). Created if missing.
    #[arg(long, value_name = "DIR", env = "PROXYCRAB_WORKSPACE")]
    workspace: Option<PathBuf>,

    /// Override the proxy listen host for this run only.
    #[arg(long, env = "PROXYCRAB_PROXY_HOST")]
    proxy_host: Option<String>,

    /// Override the proxy listen port for this run only.
    #[arg(long, env = "PROXYCRAB_PROXY_PORT")]
    proxy_port: Option<u16>,

    /// Override the management API listen port for this run only.
    #[arg(long, env = "PROXYCRAB_API_PORT")]
    api_port: Option<u16>,

    /// Do not start the management HTTP API.
    #[arg(long)]
    no_api: bool,
}

#[derive(clap::Args)]
struct InstallSkillArgs {
    /// Parent directory to install into; the skill lands at
    /// <parent>/proxycrab.
    #[arg(long, value_name = "DIR", default_value = "~/.agents/skills")]
    parent: String,

    /// Replace an existing installation.
    #[arg(long)]
    overwrite: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::InstallSkill(args)) => install_skill(&args),
        Some(Command::Run(args)) => run(args).await,
        None => run(cli.run).await,
    }
}

async fn run(args: RunArgs) -> Result<()> {
    let workspace_arg = args
        .workspace
        .ok_or_else(|| anyhow::anyhow!("--workspace is required (or set PROXYCRAB_WORKSPACE)"))?;

    let log_buffer = Arc::new(LogBuffer::default());
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(BufferLayer::new(log_buffer.clone()))
        .init();

    std::fs::create_dir_all(&workspace_arg)
        .with_context(|| format!("create workspace {}", workspace_arg.display()))?;
    let workspace = workspace_arg
        .canonicalize()
        .with_context(|| format!("resolve workspace {}", workspace_arg.display()))?;
    let runtime = ProxyCrab::open(&workspace, log_buffer)
        .with_context(|| format!("open workspace {}", workspace.display()))?;

    // Command-line flags and environment variables override the workspace
    // config in memory only; the config file on disk is left untouched.
    let has_overrides =
        args.proxy_host.is_some() || args.proxy_port.is_some() || args.api_port.is_some();
    if has_overrides {
        runtime.workspace().override_config_in_memory(|config| {
            if let Some(host) = &args.proxy_host {
                config.proxy_host = host.clone();
            }
            if let Some(port) = args.proxy_port {
                config.proxy_port = port;
            }
            if let Some(port) = args.api_port {
                config.api_port = port;
            }
        });
    }

    let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);

    let mut ui_token = None;
    let http = if args.no_api {
        None
    } else {
        let token = generate_ui_token().context("generate CLI UI access token")?;
        let access = Arc::new(UiAccess::new(&token));
        let permissions = CliPermissionService::open(&workspace, Some(access.clone()))
            .context("open management API permission store")?;
        let ui_manager = manager.clone();
        let ui_workspace = workspace.to_string_lossy().into_owned();
        let ui_permissions = permissions.clone();
        let shares = SessionShareService::new();
        let share_manager = manager.clone();
        let share_service = shares.clone();
        let har_shares = HarShareService::new();
        let har_share_manager = manager.clone();
        let har_share_service = har_shares.clone();
        let handle = start_http_server_with_routes(
            manager.clone(),
            permissions,
            shares.clone(),
            har_shares,
            move |changes| {
                ui::router(ui_manager, ui_workspace, ui_permissions, access, changes)
                    .merge(proxy_crab_mgr::session_share::router(
                        share_manager,
                        share_service,
                    ))
                    .merge(proxy_crab_mgr::har_share::router(
                        har_share_manager,
                        har_share_service,
                    ))
            },
        )
        .await
        .context("start management HTTP API")?;
        ui_token = Some(token);
        Some(handle)
    };

    if let Err(error) = manager.start_proxy().await {
        if let Some(http) = http {
            http.shutdown().await;
        }
        return Err(error).context("start proxy");
    }

    let config = manager.config().await?;
    tracing::info!(
        host = config.proxy_host,
        port = config.proxy_port,
        workspace = %workspace.display(),
        "proxy listening"
    );
    if let Some(http) = &http {
        tracing::info!(
            host = %std::net::Ipv4Addr::UNSPECIFIED,
            port = config.api_port,
            "management API listening"
        );
        println!("ProxyCrab UI: http://127.0.0.1:{}/", http.port);
        println!(
            "Access token: {}",
            ui_token.as_deref().expect("UI token exists")
        );
    }

    shutdown_signal().await;
    tracing::info!("shutting down");
    if let Err(error) = manager.stop_proxy().await {
        tracing::warn!("failed to stop proxy cleanly: {error}");
    }
    if let Some(http) = http {
        http.shutdown().await;
    }
    Ok(())
}

fn generate_ui_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("read secure random bytes: {error}"))?;
    Ok(format!("pcrab_ui_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn install_skill(args: &InstallSkillArgs) -> Result<()> {
    let temp = tempfile::tempdir().context("create temporary directory")?;
    let source = temp.path().join("proxycrab");
    extract_embedded_skill(&SKILL_DIR, &source)?;
    let info = skill_install::install(&source, &args.parent, args.overwrite)
        .context("install ProxyCrab skill")?;
    println!("installed ProxyCrab skill to {}", info.target_path);
    Ok(())
}

fn extract_embedded_skill(dir: &Dir, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)
        .with_context(|| format!("create directory {}", target.display()))?;
    for subdir in dir.dirs() {
        let name = subdir
            .path()
            .file_name()
            .expect("embedded directory has a name");
        if name == "evals" {
            continue;
        }
        extract_embedded_skill(subdir, &target.join(name))?;
    }
    for file in dir.files() {
        let name = file.path().file_name().expect("embedded file has a name");
        let path = target.join(name);
        std::fs::write(&path, file.contents())
            .with_context(|| format!("write {}", path.display()))?;
        // Embedded files lose their mode; restore the executable bit on
        // scripts so the installed skill matches a repository checkout.
        #[cfg(unix)]
        if file.contents().starts_with(b"#!") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("chmod {}", path.display()))?;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.expect("install SIGINT handler"),
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("install SIGINT handler");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_contains_entrypoints_but_no_evals() {
        assert!(SKILL_DIR.get_file("SKILL.md").is_some());
        assert!(SKILL_DIR.get_dir("scripts").is_some());
        assert!(SKILL_DIR.get_dir("references").is_some());
    }
}
