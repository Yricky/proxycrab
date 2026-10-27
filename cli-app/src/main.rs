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

    /// Use this UI access key instead of generating a random token for this run.
    #[arg(long, value_name = "KEY", value_parser = parse_ui_key)]
    ui_key: Option<String>,

    /// Load a Lua redaction file once; override all bindings and disable script management.
    #[arg(long, value_name = "FILE")]
    redaction_script: Option<PathBuf>,

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

    let manager = MitmManager::new(runtime);
    let manager: Arc<dyn ProxyCrabManager> = match args.redaction_script {
        Some(path) => load_redaction_script(&manager, &path)?,
        None => manager,
    };

    let mut ui_token = None;
    let http = if args.no_api {
        None
    } else {
        let token = match args.ui_key {
            Some(key) => key,
            None => generate_ui_token().context("generate CLI UI access token")?,
        };
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

fn load_redaction_script(manager: &MitmManager, path: &Path) -> Result<Arc<MitmManager>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read redaction script {}", path.display()))?;
    manager
        .with_forced_redaction(proxy_crab_mitm::model::Script {
            name: path.to_string_lossy().into_owned(),
            content,
        })
        .with_context(|| format!("load redaction script {}", path.display()))
}

fn parse_ui_key(value: &str) -> Result<String, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("UI key must contain only visible ASCII characters, without whitespace".into());
    }
    Ok(value.to_owned())
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
    fn startup_flags_work_with_default_and_explicit_run() {
        for argv in [
            vec![
                "proxycrab-cli",
                "--workspace",
                "workspace",
                "--redaction-script",
                "mask.lua",
                "--ui-key",
                "custom-ui-key",
            ],
            vec![
                "proxycrab-cli",
                "run",
                "--workspace",
                "workspace",
                "--redaction-script",
                "mask.lua",
                "--ui-key",
                "custom-ui-key",
            ],
        ] {
            let cli = Cli::try_parse_from(argv).unwrap();
            let args = match cli.command {
                Some(Command::Run(args)) => args,
                None => cli.run,
                _ => panic!("expected run"),
            };
            assert_eq!(args.redaction_script, Some(PathBuf::from("mask.lua")));
            assert_eq!(args.ui_key.as_deref(), Some("custom-ui-key"));
        }
        assert!(
            Cli::try_parse_from(["proxycrab-cli"])
                .unwrap()
                .run
                .redaction_script
                .is_none()
        );
    }

    #[test]
    fn ui_key_rejects_invalid_authorization_values() {
        for value in [
            "", " ", "a b", "a\tb", "a\nb", "a\rb", "a\0b", "a\u{7f}b", "密钥", "a\u{a0}b",
        ] {
            assert!(Cli::try_parse_from(["proxycrab-cli", "--ui-key", value]).is_err());
        }
        let cli = Cli::try_parse_from(["proxycrab-cli"]).unwrap();
        assert!(cli.run.ui_key.is_none());
        let token = generate_ui_token().unwrap();
        assert!(token.starts_with("pcrab_ui_"));
        assert_eq!(parse_ui_key(&token).unwrap(), token);
        assert_eq!(parse_ui_key("custom_./+~=key").unwrap(), "custom_./+~=key");
    }

    #[test]
    fn startup_redaction_requires_valid_file_and_pins_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(
            &dir.path().join("workspace"),
            Arc::new(LogBuffer::default()),
        )
        .unwrap();
        let manager = MitmManager::new(runtime);
        let path = dir.path().join("mask.lua");
        assert!(load_redaction_script(&manager, &path).is_err());
        std::fs::write(&path, "this is not valid Lua!").unwrap();
        assert!(load_redaction_script(&manager, &path).is_err());
        std::fs::write(&path, "req.uri = 'https://example.com/masked'").unwrap();
        let forced = load_redaction_script(&manager, &path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(forced.reader(None, "/api/logs/{id}").is_ok());
        assert!(forced.reader(Some("missing"), "/api/logs/{id}").is_ok());
    }

    #[tokio::test]
    async fn ui_key_cannot_bypass_startup_redaction_or_manage_scripts() {
        use proxy_crab_mitm::{
            model::{NewSession, RequestData},
            storage::CaptureStore,
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        // The server handle reports the configured port, so choose a concrete free port.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        runtime
            .workspace()
            .override_config_in_memory(|config| config.api_port = port);
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let id = store
            .begin(
                "client",
                &RequestData {
                    method: "GET".into(),
                    uri: "https://example.com/secret".into(),
                    version: "HTTP/1.1".into(),
                    headers: Default::default(),
                    tags: Default::default(),
                },
                "request",
            )
            .unwrap();
        let path = dir.path().join("startup.lua");
        std::fs::write(
            &path,
            "assert(context.api == '/api/logs/{id}'); req.uri = 'https://example.com/masked'",
        )
        .unwrap();
        let manager = load_redaction_script(&MitmManager::new(runtime), &path).unwrap();
        std::fs::write(&path, "error('changed after startup')").unwrap();
        let key = Cli::try_parse_from(["proxycrab-cli", "--ui-key", "custom-ui-key"])
            .unwrap()
            .run
            .ui_key
            .unwrap();
        let access = Arc::new(UiAccess::new(&key));
        assert!(access.allows_authorization(Some("Bearer custom-ui-key")));
        assert!(!access.allows_authorization(Some("Bearer wrong-ui-key")));
        let permissions = CliPermissionService::open(dir.path(), Some(access)).unwrap();
        let server = proxy_crab_mgr::http::start_http_server(manager, permissions)
            .await
            .unwrap();
        let read_path = format!("/api/logs/{id}?session_id={}", session.id);
        for (method, path, body, expected) in [
            ("GET", read_path.as_str(), "", "200"),
            ("GET", "/api/redaction-scripts", "", "403"),
            (
                "POST",
                "/api/redaction-scripts",
                r#"{"name":"new","content":""}"#,
                "403",
            ),
            ("GET", "/api/redaction-scripts/new", "", "403"),
            (
                "PUT",
                "/api/redaction-scripts/new",
                r#"{"content":""}"#,
                "403",
            ),
            ("DELETE", "/api/redaction-scripts/new", "", "403"),
        ] {
            let mut connection = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
                .await
                .unwrap();
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            connection.write_all(request.as_bytes()).await.unwrap();
            let mut response = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                connection.read_to_string(&mut response),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                response.starts_with(&format!("HTTP/1.1 {expected}")),
                "{response}"
            );
            if expected == "200" {
                assert!(response.contains("https://example.com/masked"));
                assert!(!response.contains("secret"));
            } else {
                assert!(response.contains("redaction_management_disabled"));
                assert!(response.contains("脱敏接口管理功能已禁用"));
            }
        }
        server.shutdown().await;
    }

    #[test]
    fn embedded_skill_contains_entrypoints_but_no_evals() {
        assert!(SKILL_DIR.get_file("SKILL.md").is_some());
        assert!(SKILL_DIR.get_dir("scripts").is_some());
        assert!(SKILL_DIR.get_dir("references").is_some());
    }
}
