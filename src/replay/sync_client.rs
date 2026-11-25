//! WebSocket Synchronization Client: Connects to `TickReplay` (<ws://127.0.0.1:49210>),
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
        Self::spawn_with_shutdown(
            ws_url,
            running,
            Arc::new(tokio::sync::Notify::new()),
            cmd_tx_holder,
            on_message,
        )
    }

    /// Start the asynchronous WebSocket synchronization loop with an explicit shutdown notifier
    /// so `stop()` immediately unblocks connect, active I/O, and reconnect backoff sleeps.
    pub fn spawn_with_shutdown<F>(
        ws_url: String,
        running: Arc<AtomicBool>,
        shutdown: Arc<tokio::sync::Notify>,
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
                    log::error!("[WsSyncClient] Failed to build Tokio runtime: {e}");
                    return;
                }
            };

            rt.block_on(async move {
                while running.load(Ordering::SeqCst) {
                    log::info!("[WsSyncClient] Connecting to TickReplay WS at {ws_url}...");
                    let connect_res = tokio::select! {
                        biased;
                        () = shutdown.notified() => {
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                            continue;
                        }
                        res = tokio_tungstenite::connect_async(&ws_url) => res,
                    };

                    if !running.load(Ordering::SeqCst) {
                        break;
                    }

                    match connect_res {
                        Ok((ws_stream, _resp)) => {
                            log::info!("[WsSyncClient] Connected to TickReplay WebSocket server!");
                            let (mut write, mut read) = ws_stream.split();
                            let (cmd_tx, mut cmd_rx) =
                                tokio::sync::mpsc::unbounded_channel::<String>();
                            *cmd_tx_holder.lock() = Some(cmd_tx);

                            while running.load(Ordering::SeqCst) {
                                tokio::select! {
                                    biased;
                                    () = shutdown.notified() => {
                                        if !running.load(Ordering::SeqCst) {
                                            let _ = write.close().await;
                                            break;
                                        }
                                    }
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
                                                log::warn!("[WsSyncClient] WS read error: {e}");
                                                break;
                                            }
                                            _ => {}
                                        }
                                    }
                                    cmd_opt = cmd_rx.recv() => {
                                        match cmd_opt {
                                            Some(cmd) => {
                                                if let Err(e) = write.send(Message::Text(cmd.into())).await {
                                                    log::warn!("[WsSyncClient] WS write error: {e}");
                                                    break;
                                                }
                                            }
                                            None => break,
                                        }
                                    }
                                }
                            }

                            *cmd_tx_holder.lock() = None;
                            if running.load(Ordering::SeqCst) {
                                on_message(r#"{"status":"DISCONNECTED","is_playing":false}"#);
                            }
                        }
                        Err(e) => {
                            log::debug!(
                                "[WsSyncClient] WS connect failed ({e}), retrying in 1s..."
                            );
                        }
                    }

                    if !running.load(Ordering::SeqCst) {
                        break;
                    }

                    tokio::select! {
                        biased;
                        () = shutdown.notified() => {
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                        }
                        () = tokio::time::sleep(Duration::from_millis(1000)) => {}
                    }
                }
            });
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn connected_ws_client_exits_promptly_on_shutdown() {
        let server_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener = server_rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let connected = Arc::new(AtomicBool::new(false));
        let connected_srv = Arc::clone(&connected);

        let server_handle = std::thread::spawn(move || {
            server_rt.block_on(async move {
                if let Ok((stream, _)) = listener.accept().await {
                    if let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await {
                        connected_srv.store(true, Ordering::SeqCst);
                        // Hold connection open until client disconnects.
                        while let Some(Ok(_)) = ws.next().await {}
                    }
                }
            });
        });

        let running = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let cmd_tx_holder = Arc::new(Mutex::new(None));

        let client_handle = WsSyncClient::spawn_with_shutdown(
            format!("ws://{addr}"),
            Arc::clone(&running),
            Arc::clone(&shutdown),
            Arc::clone(&cmd_tx_holder),
            |_| {},
        );

        let wait_deadline = Instant::now() + Duration::from_secs(3);
        while !connected.load(Ordering::SeqCst) && Instant::now() < wait_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(connected.load(Ordering::SeqCst), "client must connect");

        let stop_start = Instant::now();
        running.store(false, Ordering::SeqCst);
        *cmd_tx_holder.lock() = None;
        shutdown.notify_waiters();
        shutdown.notify_one();

        client_handle.join().expect("client thread must exit cleanly");
        assert!(
            stop_start.elapsed() < Duration::from_secs(2),
            "shutdown should complete immediately, took {:?}",
            stop_start.elapsed()
        );
        let _ = server_handle.join();
    }
}
