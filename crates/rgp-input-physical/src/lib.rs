pub mod translate;

use std::collections::{HashMap, HashSet};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gilrs::{EventType, GamepadId, Gilrs};
use rgp_core::{Control, DeviceInfo, InputEvent, RgpError, SourceId};

/// Spawn the physical-gamepad polling thread.
///
/// The thread owns a `Gilrs` instance and polls it in a tight non-blocking
/// loop (1 ms sleep between polls to yield the CPU).  When a gamepad
/// disconnects, synthetic `value = 0.0` releases are emitted for every
/// control that was tracked as held at disconnect time.
///
/// # Arguments
/// * `events_tx` — channel to send `InputEvent`s on; full channel → drop, never block.
/// * `shutdown`  — send `()` or drop the sender to stop the thread.
pub fn run(
    events_tx: Sender<InputEvent>,
    shutdown: Receiver<()>,
) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new()
        .name("rgp-input-physical".into())
        .spawn(move || -> Result<(), RgpError> {
            let mut gilrs = Gilrs::new()
                .map_err(|e| RgpError::InputSource(format!("gilrs init: {e}")))?;

            // Track which controls are currently "held" per gamepad so that
            // we can emit synthetic releases on disconnect.
            let mut held: HashMap<GamepadId, HashSet<Control>> = HashMap::new();

            loop {
                // Check shutdown first.
                match shutdown.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }

                while let Some(ev) = gilrs.next_event() {
                    let source_id = source_id_for(&gilrs, ev.id);

                    if matches!(ev.event, EventType::Disconnected) {
                        // Emit synthetic 0.0-value events for every held control.
                        if let Some(set) = held.remove(&ev.id) {
                            for ctl in set {
                                let release = InputEvent {
                                    source: SourceId::Physical(source_id.clone()),
                                    control: ctl,
                                    value: 0.0,
                                    timestamp: std::time::Instant::now(),
                                };
                                if events_tx.try_send(release).is_err() {
                                    tracing::warn!(
                                        target: "rgp::input::physical",
                                        "events_tx full; dropping synthetic release"
                                    );
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(input) = translate::translate_event_type(&ev.event, &source_id) {
                        let set = held.entry(ev.id).or_default();
                        // Heuristic: treat value > 0.5 as "held".
                        // Buttons emit exactly 0.0 or 1.0; axis values vary.
                        // We track axes in held purely for clean disconnect release.
                        if input.value > 0.5 {
                            set.insert(input.control);
                        } else {
                            set.remove(&input.control);
                        }

                        if events_tx.try_send(input).is_err() {
                            tracing::warn!(
                                target: "rgp::input::physical",
                                "events_tx full; dropping event"
                            );
                        }
                    }
                }

                thread::sleep(Duration::from_millis(1));
            }

            Ok(())
        })
        .expect("spawn input-physical thread")
}

/// Return a stable string ID for the gamepad with the given `id`.
///
/// While the gamepad is connected we use its UUID (from the HID descriptor).
/// If it has already been removed from gilrs we fall back to the debug
/// representation of the `GamepadId`.
fn source_id_for(gilrs: &Gilrs, id: GamepadId) -> String {
    match gilrs.connected_gamepad(id) {
        Some(gp) => format!("uuid:{}", uuid::Uuid::from_bytes(gp.uuid())),
        None => format!("disconnected:{id:?}"),
    }
}

/// Return `DeviceInfo` for every currently-connected gamepad.
///
/// Initialises a temporary `Gilrs` instance.  Returns an empty `Vec` on
/// init failure rather than propagating an error, matching the "list is
/// best-effort" use-case.
pub fn list_connected() -> Vec<DeviceInfo> {
    let mut gilrs = match Gilrs::new() {
        Ok(g) => g,
        Err(_) => return vec![],
    };
    // Drain any pending events. On some Windows setups gilrs.gamepads()
    // returns an empty iterator until the first poll runs; this ensures
    // XInput / DirectInput devices already plugged in get enumerated.
    while gilrs.next_event().is_some() {}
    gilrs
        .gamepads()
        .filter(|(_id, gp)| gp.is_connected())
        .map(|(_id, gp)| DeviceInfo {
            id: SourceId::Physical(format!("uuid:{}", uuid::Uuid::from_bytes(gp.uuid()))),
            name: gp.name().to_string(),
            connected: true,
        })
        .collect()
}
