#![cfg_attr(not(test), windows_subsystem = "windows")]

//! `TickScope` Replay application main entry point.
//! Multi-broker FX historical tick replay comparison and visualization,
//! synchronized with `TickReplay` (<ws://127.0.0.1:49210>) and Drenhis Hive Parquet tick archives.

use eframe::egui;
use std::env;
use std::path::{Path, PathBuf};
use tick_scope::cli::CliArgs;
use tick_scope::config::load_startup_config;
use tick_scope::logging::{cleanup_console, init_logging, is_console_allocated, setup_console};
use tick_scope::replay::ReplayCoordinator;
use tick_scope::runtime::PrecisionTimerGuard;
use tick_scope::ui::dashboard::DashboardApp;
use tick_scope::ui::settings::{
    load_ui_state, resolve_ui_state_path, save_ui_state, WindowGeometryState,
};

fn resolve_tick_dir(cli_path: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_path {
        return p;
    }
    if let Ok(p) = env::var("DRENHIS_TICK_DIR") {
        return PathBuf::from(p);
    }
    let candidates = [
        r"D:\Drehis\tick",
        r"D:\Drenhis\tick",
        r"data\tick",
    ];
    for c in &candidates {
        let p = Path::new(c);
        if p.exists() {
            return p.to_path_buf();
        }
    }
    PathBuf::from(candidates[0])
}

fn resolve_ws_url(cli_url: Option<String>) -> String {
    if let Some(url) = cli_url {
        return url;
    }
    if let Ok(url) = env::var("TICK_REPLAY_WS") {
        return url;
    }
    "ws://127.0.0.1:49210".to_string()
}

fn pause_if_allocated_console() {
    if is_console_allocated() {
        eprintln!("\nPress Enter to exit...");
        let mut _buf = String::new();
        let _ = std::io::stdin().read_line(&mut _buf);
    }
}

fn main() -> eframe::Result<()> {
    let _timer_guard = PrecisionTimerGuard::new(1);

    let cli = match CliArgs::parse(env::args().skip(1)) {
        Ok(cli) => cli,
        Err(err) => {
            setup_console();
            eprintln!("Error: {err}\n");
            eprintln!("{}", CliArgs::help_text());
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };

    if cli.show_help {
        setup_console();
        println!("TickScope Replay Runner");
        println!("{}", CliArgs::help_text());
        pause_if_allocated_console();
        std::process::exit(0);
    }

    if cli.show_version {
        setup_console();
        println!("TickScope Replay v{}", env!("CARGO_PKG_VERSION"));
        pause_if_allocated_console();
        std::process::exit(0);
    }

    let _ = init_logging(cli.console, cli.log_level);
    log::info!("TickScope Replay v{} initializing...", env!("CARGO_PKG_VERSION"));

    let exe = env::current_exe().expect("Cannot locate executable");
    let cwd = env::current_dir().expect("Cannot locate working directory");
    let exe_dir = exe.parent().unwrap();

    let mut config = match load_startup_config(cli.config_path.as_deref(), exe_dir, &cwd) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("Failed to load configuration: {e}");
            if !cli.console {
                eprintln!("Failed to load configuration: {e}");
            }
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };

    if let Some(ref sym) = cli.symbol {
        let clean_sym = sym.trim();
        if !clean_sym.is_empty() {
            log::info!("CLI flag --symbol active: overriding replay symbol to '{clean_sym}'");
            let is_jpy = clean_sym.to_ascii_uppercase().ends_with("JPY");
            let pip_size = if is_jpy { 0.01 } else { 0.0001 };
            let point_size = if is_jpy { 0.001 } else { 0.00001 };
            for b in &mut config.brokers {
                b.symbol = clean_sym.to_string();
                b.pip_size = pip_size;
                b.point_size = point_size;
            }
        }
    }

    let tick_dir = resolve_tick_dir(cli.tick_dir.clone());
    log::info!("Using historical tick directory: {}", tick_dir.display());

    let ui_state_path = resolve_ui_state_path(exe_dir, &cwd);
    let mut ui_state = load_ui_state(&ui_state_path).unwrap_or_default();
    if cli.reset_window {
        log::info!("CLI flag --reset-window active: resetting window geometry");
        ui_state.window = WindowGeometryState::default();
        let _ = save_ui_state(&ui_state_path, &ui_state);
    }
    ui_state.reconcile_with_brokers_and_config(
        &config.brokers,
        config.active_pair,
        config.mt5.non_minimized_broker.as_deref(),
    );
    config.active_pair = ui_state.active_pair;

    let initial_pair = ui_state.active_pair;
    let pip_size = config.brokers.first().map_or(0.01, |b| b.pip_size);
    let visible_seconds = config.display.visible_seconds;
    let visible_ticks = config.display.visible_ticks;
    let chart_max_quote_age_ms = config.display.chart_max_quote_age_ms;

    let ws_url = resolve_ws_url(cli.ws_url.clone());
    let mut coordinator = match ReplayCoordinator::new(config.clone(), &tick_dir, &ws_url) {
        Ok(coord) => coord,
        Err(e) => {
            log::error!("Fatal error initializing ReplayCoordinator: {e}");
            eprintln!("Fatal error initializing ReplayCoordinator: {e}");
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };

    let exchange = coordinator.exchange.clone();
    let app = DashboardApp::new(exchange, initial_pair)
        .with_pip_size(pip_size)
        .with_visible_seconds(visible_seconds)
        .with_visible_ticks(visible_ticks)
        .with_chart_max_quote_age_ms(chart_max_quote_age_ms)
        .with_broker_configs(coordinator.config.brokers.clone())
        .with_mt5_config(coordinator.config.mt5.clone())
        .with_ui_state(&ui_state)
        .with_ui_state_path(ui_state_path)
        .with_pair_selection_handler(coordinator.pair_selection_handler())
        .with_trade_store(coordinator.trade_store.clone());

    let title = format!(
        "TickScope [REPLAY] - Multi-Broker FX Historical Replay Scope ({})",
        cli.symbol.as_deref().unwrap_or("USDJPY")
    );
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(ui_state.window.inner_size)
        .with_min_inner_size([800.0, 500.0])
        .with_maximized(ui_state.window.maximized);

    if let Some(icon) = tick_scope::ui::load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    if let Some(pos) = ui_state.window.position {
        viewport = viewport.with_position(egui::pos2(pos[0], pos[1]));
    }
    if ui_state.always_on_top {
        viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnTop);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    log::info!("TickScope Replay starting GUI...");
    let result = eframe::run_native(
        "TickScope Replay",
        native_options,
        Box::new(|cc| {
            tick_scope::ui::setup_fonts(&cc.egui_ctx);
            let mut app = app;
            app.mark_fonts_configured();
            Ok(Box::new(app))
        }),
    );

    if let Err(ref e) = result {
        log::error!("eframe::run_native failed: {e:?}");
    }

    coordinator.stop();
    coordinator.wait_for_shutdown();
    cleanup_console();
    result
}
