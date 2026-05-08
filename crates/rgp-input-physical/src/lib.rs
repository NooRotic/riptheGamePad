pub mod translate;

use std::collections::{HashMap, HashSet};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gilrs::{EventType, GamepadId, Gilrs};
use rgp_core::{Control, DeviceInfo, InputEvent, RgpError, SourceId};

/// Synthesize a stable source-id string for a gamepad. XInput devices return
/// all-zero UUIDs from gilrs (XInput exposes slots, not stable per-device IDs);
/// for those we emit `xinput:<slot>`. Non-XInput devices use the gilrs UUID.
fn synthesize_source_id(uuid_bytes: [u8; 16], slot: usize) -> String {
    if uuid_bytes == [0u8; 16] {
        format!("xinput:{slot}")
    } else {
        format!("uuid:{}", uuid::Uuid::from_bytes(uuid_bytes))
    }
}

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
/// For connected gamepads, synthesize an XInput slot or UUID as appropriate.
/// If it has already been removed from gilrs, fall back to a disconnected marker.
fn source_id_for(gilrs: &Gilrs, id: GamepadId) -> String {
    let slot = usize::from(id);
    match gilrs.connected_gamepad(id) {
        Some(gp) => synthesize_source_id(gp.uuid(), slot),
        None => format!("disconnected:{slot}"),
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
        Err(gilrs::Error::NotImplemented(g)) => g,
        Err(_) => return vec![],
    };
    // gilrs's Windows backend polls XInput on a background thread; the
    // initial scan races with gamepads() if we enumerate immediately.
    // Poll in 50ms cycles up to 500ms total; exit early as soon as any
    // device is reported. Required for already-connected devices to show.
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        while gilrs.next_event().is_some() {}
        if gilrs.gamepads().any(|(_, gp)| gp.is_connected()) {
            break;
        }
    }
    gilrs
        .gamepads()
        .filter(|(_id, gp)| gp.is_connected())
        .map(|(id, gp)| {
            let slot = usize::from(id);
            DeviceInfo {
                id: SourceId::Physical(synthesize_source_id(gp.uuid(), slot)),
                name: gp.name().to_string(),
                connected: true,
            }
        })
        .collect()
}

#[cfg(test)]
mod synthesize_tests {
    use super::synthesize_source_id;

    #[test]
    fn zero_uuid_returns_xinput_slot() {
        assert_eq!(synthesize_source_id([0u8; 16], 0), "xinput:0");
        assert_eq!(synthesize_source_id([0u8; 16], 1), "xinput:1");
        assert_eq!(synthesize_source_id([0u8; 16], 3), "xinput:3");
    }

    #[test]
    fn nonzero_uuid_returns_uuid_format() {
        let mut bytes = [0u8; 16];
        bytes[0] = 0xab;
        let id = synthesize_source_id(bytes, 0);
        assert!(id.starts_with("uuid:"));
        assert!(id.contains("ab"));
    }

    #[test]
    fn slot_index_ignored_when_uuid_nonzero() {
        let mut bytes = [0u8; 16];
        bytes[15] = 0x42;
        assert_eq!(
            synthesize_source_id(bytes, 0),
            synthesize_source_id(bytes, 7),
        );
    }
}
