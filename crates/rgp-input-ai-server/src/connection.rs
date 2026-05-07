use std::time::Duration;
use rgp_core::{ButtonId, AxisId, TriggerId, InputEvent};
use rgp_input_ai::handle as ai_handle;
use rgp_input_ai::AiInputHandle;
use crate::frame::Frame;
use tokio::net::TcpStream;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::StreamExt;

pub async fn handle_connection(
    stream: TcpStream,
    events_tx: crossbeam_channel::Sender<InputEvent>,
) {
    let mut ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(?e, "ws handshake failed");
            return;
        }
    };

    let mut client_id = uuid::Uuid::new_v4().to_string();
    let mut malformed_run: u32 = 0;
    let mut handle: Option<AiInputHandle> = None;
    let mut received_first_non_hello = false;

    while let Some(msg) = ws.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };

        match serde_json::from_str::<Frame>(&text) {
            Ok(Frame::Hello { client_id: cid }) => {
                if received_first_non_hello {
                    tracing::warn!(target: "rgp::input::ai_server",
                                   "hello received after first non-hello frame; dropping");
                    continue;
                }
                client_id = cid.clone();
                tracing::debug!(target: "rgp::input::ai_server", client_id = %cid, "client identified");
                malformed_run = 0;
            }
            Ok(other) => {
                received_first_non_hello = true;
                if handle.is_none() {
                    handle = Some(ai_handle(events_tx.clone(), client_id.clone()));
                }
                let h = handle.as_ref().unwrap();
                apply(other, h);
                malformed_run = 0;
            }
            Err(e) => {
                tracing::warn!(target: "rgp::input::ai_server", ?e, "malformed frame");
                malformed_run += 1;
                if malformed_run >= 3 {
                    tracing::info!(target: "rgp::input::ai_server",
                                   "closing connection after 3 malformed frames");
                    let _ = ws.close(None).await;
                    break;
                }
            }
        }
    }

    if let Some(h) = handle.take() {
        h.release_all();
        tracing::debug!(target: "rgp::input::ai_server", client_id = %client_id, "released all held buttons");
    }
}

fn apply(f: Frame, h: &AiInputHandle) {
    match f {
        Frame::Press { button, duration_ms } => {
            if let Ok(b) = parse_button(&button) {
                h.press(b, Duration::from_millis(duration_ms));
            } else {
                tracing::warn!(target: "rgp::input::ai_server", button, "unknown button");
            }
        }
        Frame::Release { button } => {
            if let Ok(b) = parse_button(&button) {
                h.release(b);
            }
        }
        Frame::Axis { axis, value } => {
            if let Ok(a) = parse_axis(&axis) {
                h.axis(a, value);
            }
        }
        Frame::Trigger { trigger, value } => {
            if let Ok(t) = parse_trigger(&trigger) {
                h.trigger(t, value);
            }
        }
        Frame::Hello { .. } => {}
    }
}

fn parse_button(s: &str) -> Result<ButtonId, ()> {
    match s {
        "South" => Ok(ButtonId::South), "East" => Ok(ButtonId::East),
        "North" => Ok(ButtonId::North), "West" => Ok(ButtonId::West),
        "DPadUp" => Ok(ButtonId::DPadUp), "DPadDown" => Ok(ButtonId::DPadDown),
        "DPadLeft" => Ok(ButtonId::DPadLeft), "DPadRight" => Ok(ButtonId::DPadRight),
        "LeftStickClick" => Ok(ButtonId::LeftStickClick),
        "RightStickClick" => Ok(ButtonId::RightStickClick),
        "LeftBumper" => Ok(ButtonId::LeftBumper),
        "RightBumper" => Ok(ButtonId::RightBumper),
        "Start" => Ok(ButtonId::Start), "Select" => Ok(ButtonId::Select), "Guide" => Ok(ButtonId::Guide),
        _ => Err(()),
    }
}

fn parse_axis(s: &str) -> Result<AxisId, ()> {
    match s {
        "LeftStickX" => Ok(AxisId::LeftStickX), "LeftStickY" => Ok(AxisId::LeftStickY),
        "RightStickX" => Ok(AxisId::RightStickX), "RightStickY" => Ok(AxisId::RightStickY),
        _ => Err(()),
    }
}

fn parse_trigger(s: &str) -> Result<TriggerId, ()> {
    match s {
        "L2" => Ok(TriggerId::L2),
        "R2" => Ok(TriggerId::R2),
        _ => Err(()),
    }
}
