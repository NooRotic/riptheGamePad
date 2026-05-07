pub mod frame;
pub mod connection;

use std::net::SocketAddr;
use std::thread::{self, JoinHandle};
use crossbeam_channel::{Sender, Receiver};
use rgp_core::{InputEvent, RgpError};
use tokio::sync::Notify;
use std::sync::Arc;

pub fn run(
    events_tx: Sender<InputEvent>,
    addr: SocketAddr,
    shutdown: Receiver<()>,
) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-input-ai-server".into()).spawn(move || -> Result<(), RgpError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| RgpError::InputSource(format!("tokio: {e}")))?;
        let local = tokio::task::LocalSet::new();
        let notify = Arc::new(Notify::new());
        let notify_bg = notify.clone();

        // Bridge crossbeam shutdown → tokio Notify via a watcher thread.
        thread::spawn(move || {
            let _ = shutdown.recv();
            notify_bg.notify_one();
        });

        local.block_on(&rt, async move {
            let listener = tokio::net::TcpListener::bind(addr).await
                .map_err(RgpError::Io)?;
            tracing::info!(target: "rgp::input::ai_server", %addr, "websocket server listening");

            loop {
                tokio::select! {
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((stream, peer)) => {
                                tracing::debug!(target: "rgp::input::ai_server", ?peer, "ws conn");
                                let tx = events_tx.clone();
                                tokio::task::spawn_local(async move {
                                    connection::handle_connection(stream, tx).await;
                                });
                            }
                            Err(e) => {
                                tracing::warn!(target: "rgp::input::ai_server", ?e, "accept error");
                            }
                        }
                    }
                    _ = notify.notified() => {
                        tracing::info!(target: "rgp::input::ai_server", "shutdown signaled");
                        break;
                    }
                }
            }
            Ok::<_, RgpError>(())
        })?;
        Ok(())
    }).expect("spawn ai-server thread")
}
