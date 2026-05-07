use std::path::PathBuf;
use std::time::Duration;
use clap::Parser;
use crossbeam_channel::bounded;
use directories::ProjectDirs;
use rgp_core::RgpError;

#[derive(Parser)]
#[command(name = "riptheGamePad", about = "Virtual gamepad mixer for fight sticks and AI agents")]
struct Args {
    #[arg(long, help = "Path to config TOML (default: %APPDATA%/riptheGamePad/config.toml)")]
    config: Option<PathBuf>,

    #[arg(long, help = "Print connected gamepad UUIDs and exit")]
    list_devices: bool,
}

fn config_path(args: &Args) -> PathBuf {
    args.config.clone().unwrap_or_else(|| {
        ProjectDirs::from("com", "nooroticx", "riptheGamePad")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    })
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("RGP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    if args.list_devices {
        let devices = rgp_input_physical::list_connected();
        if devices.is_empty() {
            println!("No gamepads detected. Plug in a controller and try again.");
        } else {
            println!("Connected gamepads:");
            for d in devices {
                let uuid = match d.id {
                    rgp_core::SourceId::Physical(s) => s,
                    _ => "(unknown)".to_string(),
                };
                println!("  {} → {}", uuid, d.name);
            }
            println!();
            println!("Add an entry to [devices] in your config.toml:");
            println!("  fight_stick = \"<one of the uuids above>\"");
        }
        return;
    }

    let cfg_path = config_path(&args);
    tracing::info!(path = %cfg_path.display(), "loading config");
    let config = match ensure_config_exists(&cfg_path).and_then(|()| rgp_config::load(&cfg_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error at {}: {e}", cfg_path.display());
            std::process::exit(1);
        }
    };

    // Probe ViGEmBus before spawning workers.
    let pad = match rgp_virtual_pad::connect() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(?e, "ViGEmBus probe failed");
            // For v1: print error and exit. A future improvement is to run the
            // tray in red-error mode without input/router threads.
            eprintln!("ViGEmBus error: {e}");
            eprintln!("Install ViGEmBus from https://github.com/ViGEm/ViGEmBus/releases and try again.");
            std::process::exit(2);
        }
    };

    let (events_tx,  events_rx)  = bounded(1024);
    let (pad_tx,     pad_rx)     = bounded(256);
    let (control_tx, control_rx) = bounded(64);
    let (shutdown_tx, shutdown_rx) = bounded::<()>(0);

    let h_pad   = rgp_virtual_pad::run(pad_rx, shutdown_rx.clone(), Box::new(pad));
    let h_rtr   = rgp_router::run(events_rx, control_rx, pad_tx, config.clone(), shutdown_rx.clone());
    let h_phys  = rgp_input_physical::run(events_tx.clone(), shutdown_rx.clone());
    let h_ai    = rgp_input_ai_server::run(events_tx.clone(), config.server.addr, shutdown_rx.clone());

    let profile_ids: Vec<_> = config.profiles.iter().map(|p| p.id.clone()).collect();
    if let Err(e) = rgp_tray::run_on_main(control_tx, profile_ids, config.hotkeys.clone()) {
        tracing::error!(?e, "tray error");
    }
    drop(shutdown_tx);

    join_with_timeout(h_pad, "virtual-pad");
    join_with_timeout(h_rtr, "router");
    join_with_timeout(h_phys, "input-physical");
    join_with_timeout(h_ai,  "input-ai-server");
}

const DEFAULT_CONFIG: &str = include_str!("../../../assets/config.default.toml");

fn ensure_config_exists(path: &std::path::Path) -> Result<(), RgpError> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(RgpError::Io)?;
    }
    std::fs::write(path, DEFAULT_CONFIG).map_err(RgpError::Io)?;
    tracing::info!(target: "riptheGamePad", path = %path.display(),
                   "wrote default config template");
    Ok(())
}

fn join_with_timeout(h: std::thread::JoinHandle<Result<(), RgpError>>, name: &str) {
    let start = std::time::Instant::now();
    while !h.is_finished() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(50));
    }
    if h.is_finished() {
        match h.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(thread = name, ?e, "thread error"),
            Err(_) => tracing::error!(thread = name, "thread panicked"),
        }
    } else {
        tracing::error!(thread = name, "did not exit cleanly within 2s");
    }
}
