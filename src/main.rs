//! TickScope application main entry point.
//! Multi-broker FX real-time tick comparison and candlestick chart visualization.

use eframe::egui;
use std::env;
use std::path::Path;
use tick_compare::config::load_startup_config;
use tick_compare::runtime::coordinator::RuntimeCoordinator;
use tick_compare::ui::dashboard::DashboardApp;
use tick_compare::ui::settings::{load_ui_state, resolve_ui_state_path};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    println!("TickScope initializing...");
    let exe = env::current_exe().expect("Cannot locate TickScope executable");
    let cwd = env::current_dir().expect("Cannot locate working directory");
    let exe_dir = exe.parent().unwrap();
    let mut config = match load_startup_config(args.get(1).map(Path::new), exe_dir, &cwd) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
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
            eprintln!("Fatal error starting RuntimeCoordinator: {}", e);
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

    println!("TickScope running. Starting egui GUI window...");
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
    result
}
