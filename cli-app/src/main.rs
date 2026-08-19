mod permissions;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use include_dir::{Dir, include_dir};
use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager, http::start_http_server, skill_install,
};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::{BufferLayer, LogBuffer},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::permissions::CliPermissionService;

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

    /// Override the management API listen host for this run only. Falls back
    /// to PROXYCRAB_API_URL when neither this flag nor its env var is set.
    #[arg(long, env = "PROXYCRAB_API_HOST")]
    api_host: Option<String>,

    /// Override the management API listen port for this run only. Falls back
    /// to PROXYCRAB_API_URL when neither this flag nor its env var is set.
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

async fn run(mut args: RunArgs) -> Result<()> {
    apply_api_url_env(&mut args)?;
    let workspace_arg = args.workspace.ok_or_else(|| {
        anyhow::anyhow!("--workspace is required (or set PROXYCRAB_WORKSPACE)")
    })?;

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
    let runtime = ProxyCrab::open_workspace(&workspace, log_buffer)
        .with_context(|| format!("open workspace {}", workspace.display()))?;

    // Command-line flags and environment variables override the workspace
    // config in memory only; the config file on disk is left untouched.
    let has_overrides = args.proxy_host.is_some()
        || args.proxy_port.is_some()
        || args.api_host.is_some()
        || args.api_port.is_some();
    if has_overrides {
        runtime.workspace().override_config_in_memory(|config| {
            if let Some(host) = &args.proxy_host {
                config.proxy_host = host.clone();
            }
            if let Some(port) = args.proxy_port {
                config.proxy_port = port;
            }
            if let Some(host) = &args.api_host {
                config.api_host = host.clone();
            }
            if let Some(port) = args.api_port {
                config.api_port = port;
            }
        });
    }

    let manager: Arc<dyn ProxyCrabManager> = MitmManager::new(runtime);

    let http = if args.no_api {
        None
    } else {
        let permissions = CliPermissionService::open(&workspace)
            .context("open management API permission store")?;
        Some(
            start_http_server(manager.clone(), permissions)
                .await
                .context("start management HTTP API")?,
        )
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
    if http.is_some() {
        tracing::info!(
            host = config.api_host,
            port = config.api_port,
            "management API listening"
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

/// PROXYCRAB_API_URL (aligned with the agent skill's bundled scripts) fills in
/// the management API host/port when they were not given via flags or the
/// dedicated env vars.
fn apply_api_url_env(args: &mut RunArgs) -> Result<()> {
    if args.api_host.is_some() && args.api_port.is_some() {
        return Ok(());
    }
    let Ok(raw) = std::env::var("PROXYCRAB_API_URL") else {
        return Ok(());
    };
    if raw.trim().is_empty() {
        return Ok(());
    }
    let url = url::Url::parse(&raw)
        .with_context(|| format!("invalid PROXYCRAB_API_URL: {raw}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("PROXYCRAB_API_URL must use http or https: {raw}");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("PROXYCRAB_API_URL has no host: {raw}"))?;
    if args.api_host.is_none() {
        args.api_host = Some(host.to_owned());
    }
    if args.api_port.is_none() {
        args.api_port = url.port_or_known_default();
    }
    Ok(())
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
    fn api_url_env_fills_missing_host_and_port() {
        let mut args = RunArgs {
            workspace: None,
            proxy_host: None,
            proxy_port: None,
            api_host: None,
            api_port: None,
            no_api: false,
        };
        // SAFETY: test process env mutation; tests using it must not run
        // concurrently with other env-dependent code.
        unsafe { std::env::set_var("PROXYCRAB_API_URL", "http://127.0.0.1:19001") };
        apply_api_url_env(&mut args).unwrap();
        unsafe { std::env::remove_var("PROXYCRAB_API_URL") };
        assert_eq!(args.api_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(args.api_port, Some(19001));
    }

    #[test]
    fn embedded_skill_contains_entrypoints_but_no_evals() {
        assert!(SKILL_DIR.get_file("SKILL.md").is_some());
        assert!(SKILL_DIR.get_dir("scripts").is_some());
        assert!(SKILL_DIR.get_dir("references").is_some());
    }
}
