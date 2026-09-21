use std::path::Path;
use std::thread;
use std::time::Duration;
use tick_compare::config::load_config_from_file;
use tick_compare::contracts::ports::SnapshotExchangePort;
use tick_compare::contracts::types::ConnectionState;
use tick_compare::runtime::coordinator::RuntimeCoordinator;

#[test]
#[ignore = "Live MT5 end-to-end connection test (requires running MT5 terminals)"]
fn test_live_mt5_continuous_connection() {
    let config_path = Path::new("config/default.toml");
    let config = match load_config_from_file(config_path) {
        Ok(c) => c,
        Err(_) => return, // skip if not running locally
    };

    println!("Starting RuntimeCoordinator on ports 39001, 39002...");
    let mut coordinator = match RuntimeCoordinator::new(config) {
        Ok(c) => c,
        Err(e) => {
            println!("Could not bind to ports (may be in use): {}", e);
            return;
        }
    };

    println!("Waiting for MT5 terminals to connect and stream for 6 seconds...");
    for i in 1..=6 {
        thread::sleep(Duration::from_secs(1));
        let snapshot = coordinator.exchange.load_latest();
        println!("--- Second {} ---", i);
        for b in &snapshot.broker_overviews {
            let conn = match b.health.connection {
                ConnectionState::Connected => "Connected",
                ConnectionState::Connecting => "Connecting",
                ConnectionState::Disconnected => "Disconnected",
            };
            let quote = if let Some(q) = &b.latest_quote {
                format!("Bid={:.3}, Ask={:.3}", q.bid, q.ask)
            } else {
                "No Quote".to_string()
            };
            println!("Broker #{}: {} ({}) | State: {} | Rate: {:.1} t/s | {}",
                b.broker_id, b.name, b.symbol, conn, b.tick_rate_1s, quote);
        }
    }

    coordinator.stop();
    println!("Live test finished cleanly.");
}
