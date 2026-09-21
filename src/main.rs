//! TickCompare application main entry point.
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

    println!("TickCompare initializing...");
    println!("Loading config from: {}", config_path);

    let config = match load_config_from_file(Path::new(&config_path)) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Warning: Failed to load '{}': {}. Falling back to default configuration.", config_path, e);
            AppConfig::default()
        }
    };

    let initial_pair = config.active_pair;
    let coordinator = match RuntimeCoordinator::new(config) {
        Ok(coord) => coord,
        Err(e) => {
            eprintln!("Fatal error starting RuntimeCoordinator: {}", e);
            std::process::exit(1);
        }
    };

    let exchange = coordinator.exchange.clone();
    let app = DashboardApp::new(exchange, initial_pair);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TickCompare - Multi-Broker Real-time FX Tick Scope")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    println!("TickCompare running. Starting egui GUI window...");
    eframe::run_native(
        "TickCompare",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
