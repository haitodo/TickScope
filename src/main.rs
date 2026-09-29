#![cfg_attr(not(test), windows_subsystem = "windows")]

//! TickScope application main entry point.
//! Multi-broker FX real-time tick comparison and candlestick chart visualization.

use eframe::egui;
use std::env;
use tick_scope::cli::CliArgs;
use tick_scope::config::load_startup_config;
use tick_scope::logging::{cleanup_console, init_logging, is_console_allocated, setup_console};
use tick_scope::runtime::coordinator::RuntimeCoordinator;
use tick_scope::ui::dashboard::DashboardApp;
use tick_scope::ui::settings::{
    load_ui_state, resolve_ui_state_path, save_ui_state, WindowGeometryState,
};

fn pause_if_allocated_console() {
    if is_console_allocated() {
        eprintln!("\nPress Enter to exit...");
        let mut _buf = String::new();
        let _ = std::io::stdin().read_line(&mut _buf);
    }
}

fn main() -> eframe::Result<()> {
    let cli = match CliArgs::parse(env::args().skip(1)) {
        Ok(cli) => cli,
        Err(err) => {
            setup_console();
            eprintln!("Error: {}\n", err);
            eprintln!("{}", CliArgs::help_text());
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };

    if cli.show_help {
        setup_console();
        println!("{}", CliArgs::help_text());
        pause_if_allocated_console();
        std::process::exit(0);
    }

    if cli.show_version {
        setup_console();
        println!("TickScope {}", env!("CARGO_PKG_VERSION"));
        pause_if_allocated_console();
        std::process::exit(0);
    }

    // Initialize diagnostic logging and attach/alloc console if requested.
    // If cli.console is false, logging is completely disabled (LevelFilter::Off)
    // and no console window appears.
    let _ = init_logging(cli.console, cli.log_level);

    log::info!("TickScope v{} initializing...", env!("CARGO_PKG_VERSION"));
    let exe = env::current_exe().expect("Cannot locate TickScope executable");
    let cwd = env::current_dir().expect("Cannot locate working directory");
    let exe_dir = exe.parent().unwrap();
    let mut config = match load_startup_config(cli.config_path.as_deref(), exe_dir, &cwd) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("Failed to load configuration: {}", e);
            if !cli.console {
                eprintln!("Failed to load configuration: {}", e);
            }
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };
    if cli.record_raw {
        config.logger.enabled = true;
    }
    if config.logger.enabled {
        log::info!(
            "Raw frame capture enabled (directory: {})",
            config.logger.log_dir
        );
    }

    let ui_state_path = resolve_ui_state_path(exe_dir, &cwd);
    let mut ui_state = load_ui_state(&ui_state_path).unwrap_or_default();
    if cli.reset_window {
        log::info!("CLI flag --reset-window active: resetting window geometry to defaults");
        ui_state.window = WindowGeometryState::default();
        let _ = save_ui_state(&ui_state_path, &ui_state);
    }
    ui_state.reconcile_with_brokers_and_config(
        &config.brokers,
        config.active_pair,
        config.mt5.non_minimized_broker.as_deref(),
    );

    // Ensure the engine starts with the restored active pair
    config.active_pair = ui_state.active_pair;

    let initial_pair = ui_state.active_pair;
    let pip_size = config.brokers.first().map(|b| b.pip_size).unwrap_or(0.01);
    let visible_seconds = config.display.visible_seconds;
    let visible_ticks = config.display.visible_ticks;
    let chart_max_quote_age_ms = config.display.chart_max_quote_age_ms;
    let mut coordinator = match RuntimeCoordinator::new_with_diagnostics(
        config,
        cli.diagnostics,
    ) {
        Ok(coord) => coord,
        Err(e) => {
            log::error!("Fatal error starting RuntimeCoordinator: {}", e);
            if !cli.console {
                eprintln!("Fatal error starting RuntimeCoordinator: {}", e);
            }
            pause_if_allocated_console();
            std::process::exit(1);
        }
    };

    // Discover MT5 terminals for auto-deployment and process lifecycle management
    let (discovered_terminals, _) = tick_scope::deploy::discover_mt5_terminals(&coordinator.config.mt5);

    // Deploy the connection map only after each listener has reserved its
    // actual OS-selected port. Every terminal then receives usable endpoints.
    if coordinator.config.mt5.auto_deploy {
        let deploy_report = tick_scope::deploy::deploy_mt5_files_for_brokers(
            &coordinator.config.mt5,
            &coordinator.deployment_brokers,
        );
        tick_scope::deploy::print_deploy_report(&deploy_report);
    }

    let should_auto_launch = ui_state.mt5_auto_launch || coordinator.config.mt5.auto_launch_terminals;
    let should_auto_close = ui_state.mt5_auto_close || coordinator.config.mt5.auto_close_terminals;

    if should_auto_launch && !ui_state.mt5_launch_targets.is_empty() {
        log::info!(
            "[MT5 Auto-Launch] Auto-launching {} MT5 terminal(s)...",
            ui_state.mt5_launch_targets.len()
        );
        let mut tm = tick_scope::runtime::TerminalManager::new();
        tm.poll_status(&coordinator.config.brokers, &discovered_terminals, true);
        let stopped_targets: Vec<_> = ui_state
            .mt5_launch_targets
            .iter()
            .copied()
            .filter(|&id| !tm.get_status(id).is_running())
            .collect();
        let minimized = ui_state.mt5_minimized;
        let normal_id = ui_state.mt5_non_minimized_broker;
        let _ = tm.launch_multiple_with_normal(&stopped_targets, normal_id, minimized);
    }

    let exchange = coordinator.exchange.clone();
    let app = DashboardApp::new(exchange, initial_pair)
        .with_pip_size(pip_size)
        .with_visible_seconds(visible_seconds)
        .with_visible_ticks(visible_ticks)
        .with_chart_max_quote_age_ms(chart_max_quote_age_ms)
        .with_broker_configs(coordinator.config.brokers.clone())
        .with_mt5_config(coordinator.config.mt5.clone())
        .with_discovered_terminals(discovered_terminals.clone())
        .with_ui_state(&ui_state)
        .with_ui_state_path(ui_state_path)
        .with_pair_selection_handler(coordinator.pair_selection_handler());
    let app = if let Some(diagnostics) = coordinator.diagnostics_handle() {
        app.with_diagnostics(diagnostics, coordinator.clock.clone())
    } else {
        app
    };

    // Use saved logical inner size directly.  winit (Per-Monitor DPI V2
    // on Windows) automatically scales the physical pixel count to preserve
    // the same visual (inch) size on any monitor, so no manual DPI
    // adjustment is needed at startup.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("TickScope - Multi-Broker Real-time FX Tick Scope")
        .with_inner_size(ui_state.window.inner_size)
        .with_min_inner_size([800.0, 500.0])
        .with_maximized(ui_state.window.maximized);

    if let Some(pos) = ui_state.window.position {
        viewport = viewport.with_position(egui::pos2(pos[0], pos[1]));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    log::info!("TickScope running. Starting egui GUI window...");
    let result = eframe::run_native(
        "TickScope",
        native_options,
        Box::new(|cc| {
            tick_scope::ui::setup_fonts(&cc.egui_ctx);
            let mut app = app;
            app.mark_fonts_configured();
            Ok(Box::new(app))
        }),
    );

    if should_auto_close {
        log::info!("[MT5 Auto-Close] Auto-closing running MT5 terminals...");
        let mut tm = tick_scope::runtime::TerminalManager::new();
        tm.poll_status(&coordinator.config.brokers, &discovered_terminals, true);
        let running_targets: Vec<_> = ui_state
            .mt5_launch_targets
            .iter()
            .copied()
            .filter(|&id| tm.get_status(id).is_running())
            .collect();
        tm.stop_multiple(&running_targets, std::time::Duration::from_secs(5));
    }

    coordinator.stop();
    coordinator.wait_for_shutdown();
    cleanup_console();
    result

}
