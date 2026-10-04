//! WebSocket Synchronization Client: Connects to TickReplay (ws://127.0.0.1:49210),
//! handles robust auto-reconnection, and multiplexes bi-directional command/status streams.

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

pub struct WsSyncClient;

impl WsSyncClient {
    /// Start the asynchronous WebSocket synchronization loop in a background thread.
    pub fn spawn<F>(
        ws_url: String,
        running: Arc<AtomicBool>,
        cmd_tx_holder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
        on_message: F,
    ) -> std::thread::JoinHandle<()>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let on_message = Arc::new(on_message);

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("[WsSyncClient] Failed to build Tokio runtime: {}", e);
                    return;
                }
            };

            rt.block_on(async move {
                while running.load(Ordering::SeqCst) {
                    log::info!("[WsSyncClient] Connecting to TickReplay WS at {}...", ws_url);
                    match tokio_tungstenite::connect_async(&ws_url).await {
                        Ok((ws_stream, _resp)) => {
                            log::info!("[WsSyncClient] Connected to TickReplay WebSocket server!");
                            let (mut write, mut read) = ws_stream.split();
                            let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                            *cmd_tx_holder.lock() = Some(cmd_tx);

                            loop {
                                tokio::select! {
                                    msg_opt = read.next() => {
                                        match msg_opt {
                                            Some(Ok(Message::Text(text))) => {
                                                on_message(&text);
                                            }
                                            Some(Ok(Message::Ping(p))) => {
                                                let _ = write.send(Message::Pong(p)).await;
                                            }
                                            Some(Ok(Message::Close(_))) | None => {
                                                log::warn!("[WsSyncClient] TickReplay WS connection closed.");
                                                break;
                                            }
                                            Some(Err(e)) => {
                                                log::warn!("[WsSyncClient] WS read error: {}", e);
                                                break;
                                            }
                                            _ => {}
                                        }
                                    }
                                    cmd_opt = cmd_rx.recv() => {
                                        match cmd_opt {
                                            Some(cmd) => {
                                                if let Err(e) = write.send(Message::Text(cmd.into())).await {
                                                    log::warn!("[WsSyncClient] WS write error: {}", e);
                                                    break;
                                                }
                                            }
                                            None => break,
                                        }
                                    }
                                }
                            }

                            *cmd_tx_holder.lock() = None;
                        }
                        Err(e) => {
                            log::debug!("[WsSyncClient] WS connect failed ({}), retrying in 1s...", e);
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            });
        })
    }
}
