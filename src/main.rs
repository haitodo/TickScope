#![cfg_attr(not(test), windows_subsystem = "windows")]

//! TickScope application main entry point.
//! Multi-broker FX real-time tick comparison and candlestick chart visualization.

use eframe::egui;
use std::env;
use tick_compare::cli::CliArgs;
use tick_compare::config::load_startup_config;
use tick_compare::logging::{cleanup_console, init_logging, is_console_allocated, setup_console};
use tick_compare::runtime::coordinator::RuntimeCoordinator;
use tick_compare::ui::dashboard::DashboardApp;
use tick_compare::ui::settings::{load_ui_state, resolve_ui_state_path};

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

    let ui_state_path = resolve_ui_state_path(exe_dir, &cwd);
    let mut ui_state = load_ui_state(&ui_state_path).unwrap_or_default();
    ui_state.reconcile_with_brokers(&config.brokers, config.active_pair);

    // Ensure the engine starts with the restored active pair
    config.active_pair = ui_state.active_pair;

    let initial_pair = ui_state.active_pair;
    let pip_size = config.brokers.first().map(|b| b.pip_size).unwrap_or(0.01);
    let visible_seconds = config.display.visible_seconds;
    let visible_ticks = config.display.visible_ticks;
    let mut coordinator = match RuntimeCoordinator::new(config) {
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

    // Deploy the connection map only after each listener has reserved its
    // actual OS-selected port. Every terminal then receives usable endpoints.
    if coordinator.config.mt5.auto_deploy {
        let deploy_report = tick_compare::runtime::deploy_mt5_files_for_brokers(
            &coordinator.config.mt5,
            &coordinator.deployment_brokers,
        );
        tick_compare::runtime::print_deploy_report(&deploy_report);
    }

    let exchange = coordinator.exchange.clone();
    let app = DashboardApp::new(exchange, initial_pair)
        .with_pip_size(pip_size)
        .with_visible_seconds(visible_seconds)
        .with_visible_ticks(visible_ticks)
        .with_ui_state(&ui_state)
        .with_ui_state_path(ui_state_path)
        .with_pair_selection_handler(coordinator.pair_selection_handler());

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
            tick_compare::ui::setup_fonts(&cc.egui_ctx);
            let mut app = app;
            app.mark_fonts_configured();
            Ok(Box::new(app))
        }),
    );

    coordinator.stop();
    coordinator.wait_for_shutdown();
    cleanup_console();
    result
}
