//! TickScope application main entry point.
//! Multi-broker FX real-time tick comparison and candlestick chart visualization.

use eframe::egui;
use std::env;
use std::path::Path;
use tick_compare::config::load_config_from_file;
use tick_compare::contracts::config::AppConfig;
use tick_compare::runtime::coordinator::RuntimeCoordinator;
use tick_compare::ui::dashboard::DashboardApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    let config_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "config/default.toml".to_string()
    };

    println!("TickScope initializing...");
    println!("Loading config from: {}", config_path);

    let config = match load_config_from_file(Path::new(&config_path)) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Warning: Failed to load '{}': {}. Falling back to default configuration.", config_path, e);
            AppConfig::default()
        }
    };

    // Auto-deploy MT5 EA and Include files to detected MT5 terminals
    if config.mt5.auto_deploy {
        let deploy_report = tick_compare::runtime::deploy_mt5_files(&config.mt5);
        tick_compare::runtime::print_deploy_report(&deploy_report);
    }

    let initial_pair = config.active_pair;
    let pip_size = config.brokers.first().map(|b| b.pip_size).unwrap_or(0.01);
    let coordinator = match RuntimeCoordinator::new(config) {
        Ok(coord) => coord,
        Err(e) => {
            eprintln!("Fatal error starting RuntimeCoordinator: {}", e);
            std::process::exit(1);
        }
    };

    let exchange = coordinator.exchange.clone();
    let app = DashboardApp::new(exchange, initial_pair).with_pip_size(pip_size);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TickScope - Multi-Broker Real-time FX Tick Scope")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    println!("TickScope running. Starting egui GUI window...");
    eframe::run_native(
        "TickScope",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
