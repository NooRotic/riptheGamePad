pub mod apply;

use std::collections::HashMap;
use std::thread::{self, JoinHandle};
use crossbeam_channel::{Sender, Receiver, select};
use rgp_core::{InputEvent, PadState, ControlMsg, RgpError, ProfileId, Control, SourceId};
use rgp_config::{Config, CompiledProfile};

pub fn run(
    events_rx: Receiver<InputEvent>,
    control_rx: Receiver<ControlMsg>,
    pad_tx: Sender<PadState>,
    config: Config,
    shutdown: Receiver<()>,
) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-router".into()).spawn(move || -> Result<(), RgpError> {
        // Pre-compile all profiles at startup so SetActiveProfile is fast and total.
        let mut compiled: HashMap<ProfileId, CompiledProfile> = HashMap::new();
        for p in &config.profiles {
            let cp = config.compile(&p.id)?;
            compiled.insert(p.id.clone(), cp);
        }
        let mut current_id = config.default_profile();
        let mut state = PadState::default();
        let mut last_seen: HashMap<(SourceId, Control), f32> = HashMap::new();

        loop {
            select! {
                recv(events_rx) -> ev => match ev {
                    Ok(event) => {
                        last_seen.insert((event.source.clone(), event.control), event.value);
                        let active = match compiled.get(&current_id) {
                            Some(cp) => cp,
                            None => {
                                tracing::error!(target: "rgp::router",
                                    profile = %current_id.0, "active profile not compiled");
                                continue;
                            }
                        };
                        if apply::apply_event(&mut state, active, &event)
                            && pad_tx.try_send(state.clone()).is_err()
                        {
                            tracing::warn!(target: "rgp::router",
                                "pad_tx full; dropping snapshot");
                        }
                    }
                    Err(_) => break,
                },
                recv(control_rx) -> msg => match msg {
                    Ok(ControlMsg::SetActiveProfile(id)) => {
                        if !compiled.contains_key(&id) {
                            tracing::warn!(target: "rgp::router",
                                profile = %id.0, "unknown profile; ignoring");
                            continue;
                        }
                        current_id = id.clone();
                        // Rebuild state from last-seen against new profile.
                        state = PadState::default();
                        let active = compiled.get(&current_id).unwrap();
                        for ((src, ctl), val) in &last_seen {
                            let synthetic = InputEvent {
                                source: src.clone(),
                                control: *ctl,
                                value: *val,
                                timestamp: std::time::Instant::now(),
                            };
                            apply::apply_event(&mut state, active, &synthetic);
                        }
                        let _ = pad_tx.try_send(state.clone());
                        tracing::info!(target: "rgp::router",
                            profile = %current_id.0, "profile activated");
                    }
                    Ok(ControlMsg::ListDevices(reply)) => {
                        let _ = reply.send(vec![]);
                    }
                    Ok(ControlMsg::PanicDisconnect) => {
                        tracing::warn!(target: "rgp::router", "panic disconnect: zeroing pad state");
                        state = PadState::default();
                        // Also clear last_seen so a profile-switch doesn't restore stuck state.
                        last_seen.clear();
                        let _ = pad_tx.try_send(state.clone());
                    }
                    Ok(ControlMsg::Quit) => break,
                    Err(_) => break,
                },
                recv(shutdown) -> _ => break,
            }
        }
        Ok(())
    }).expect("spawn router thread")
}
