//! Embedded MQL5 source code and file loaders.

use std::fs;
use std::path::PathBuf;

/// Embedded copies as reliable fallback when source repo is not on disk
pub const EMBEDDED_TICK_COLLECTOR: &str = include_str!("../../mt5/TickCollector.mq5");
pub const EMBEDDED_PROTOCOL_MQH: &str = include_str!("../../mt5/Include/TickScope/Protocol.mqh");
pub const EMBEDDED_SOCKET_CLIENT_MQH: &str =
    include_str!("../../mt5/Include/TickScope/SocketClient.mqh");

/// Discovered source files to deploy
#[derive(Debug, Clone)]
pub struct SourceFiles {
    pub tick_collector: String,
    pub protocol_mqh: String,
    pub socket_client_mqh: String,
    pub source_origin: String,
}

/// Locate source files, prioritizing live disk files and falling back to embedded code.
#[must_use]
pub fn load_source_files() -> SourceFiles {
    let candidate_dirs = [
        PathBuf::from("mt5"),
        PathBuf::from("../mt5"),
        PathBuf::from("../../mt5"),
        PathBuf::from("../../../mt5"),
    ];

    for base in &candidate_dirs {
        let ea_path = base.join("TickCollector.mq5");
        let proto_path = base.join("Include").join("TickScope").join("Protocol.mqh");
        let socket_path = base
            .join("Include")
            .join("TickScope")
            .join("SocketClient.mqh");

        if ea_path.is_file() && proto_path.is_file() && socket_path.is_file() {
            if let (Ok(ea), Ok(proto), Ok(sock)) = (
                fs::read_to_string(&ea_path),
                fs::read_to_string(&proto_path),
                fs::read_to_string(&socket_path),
            ) {
                return SourceFiles {
                    tick_collector: ea,
                    protocol_mqh: proto,
                    socket_client_mqh: sock,
                    source_origin: format!("Disk: {}", base.display()),
                };
            }
        }
    }

    SourceFiles {
        tick_collector: EMBEDDED_TICK_COLLECTOR.to_string(),
        protocol_mqh: EMBEDDED_PROTOCOL_MQH.to_string(),
        socket_client_mqh: EMBEDDED_SOCKET_CLIENT_MQH.to_string(),
        source_origin: "Embedded binary default".to_string(),
    }
}
