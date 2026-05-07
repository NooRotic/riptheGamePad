use std::sync::{Arc, Mutex};
use std::time::Duration;
use crossbeam_channel::{bounded, Sender};
use rgp_core::*;
use rgp_config::parse_str;
use rgp_router::run as router_run;
use rgp_input_ai::handle as ai_handle;
use rgp_virtual_pad::{PadSink, run as pad_run};
use vigem_client::XGamepad;

const FOUR_PROFILES_TOML: &str = r#"
[devices]
fight_stick   = "uuid:fs1"
fight_stick_2 = "uuid:fs2"
xbox_pad      = "uuid:xp1"

[[profile]]
id = "fightstick-mixer"
name = "Mixer"
inputs = ["fight_stick", "fight_stick_2"]
[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to = "passthrough"
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadUp" }
to = { axis = "RightStickY", value = -1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadDown" }
to = { axis = "RightStickY", value = 1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadLeft" }
to = { axis = "RightStickX", value = -1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadRight" }
to = { axis = "RightStickX", value = 1.0 }

[[profile]]
id = "pad-passthrough"
name = "Pad"
inputs = ["xbox_pad"]
[[profile.rule]]
from = { device = "xbox_pad", control = "*" }
to = "passthrough"

[[profile]]
id = "ai-only"
name = "AI"
inputs = ["ai:*"]
[[profile.rule]]
from = { device = "ai:*", control = "*" }
to = "passthrough"

[[profile]]
id = "fightstick-plus-ai"
name = "Copilot"
inputs = ["fight_stick", "ai:*"]
[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to = "passthrough"
[[profile.rule]]
from = { device = "ai:*", control = "*" }
to = "passthrough"

[default]
profile = "fightstick-mixer"

[server]
addr = "127.0.0.1:7779"

[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

struct FakePad {
    submitted: Arc<Mutex<Vec<XGamepad>>>,
}

impl PadSink for FakePad {
    fn submit(&mut self, r: XGamepad) -> Result<(), RgpError> {
        self.submitted.lock().unwrap().push(r);
        Ok(())
    }
}

struct Harness {
    events_tx: Sender<InputEvent>,
    control_tx: Sender<ControlMsg>,
    submitted: Arc<Mutex<Vec<XGamepad>>>,
    shutdown_tx: Option<crossbeam_channel::Sender<()>>,
    handles: Vec<std::thread::JoinHandle<Result<(), RgpError>>>,
}

impl Harness {
    fn new(toml: &str) -> Self {
        let cfg = parse_str(toml).expect("parse");
        let (events_tx, events_rx) = bounded(1024);
        let (pad_tx, pad_rx) = bounded(256);
        let (control_tx, control_rx) = bounded(64);
        let (shutdown_tx, shutdown_rx) = bounded::<()>(0);
        let submitted = Arc::new(Mutex::new(Vec::<XGamepad>::new()));
        let fake = FakePad { submitted: submitted.clone() };
        let h_pad = pad_run(pad_rx, shutdown_rx.clone(), Box::new(fake));
        let h_rtr = router_run(events_rx, control_rx, pad_tx, cfg, shutdown_rx);
        Harness {
            events_tx,
            control_tx,
            submitted,
            shutdown_tx: Some(shutdown_tx),
            handles: vec![h_pad, h_rtr],
        }
    }

    fn set_profile(&self, id: &str) {
        self.control_tx
            .send(ControlMsg::SetActiveProfile(ProfileId(id.into())))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }

    fn ai_handle(&self, source_id: &str) -> rgp_input_ai::AiInputHandle {
        ai_handle(self.events_tx.clone(), source_id)
    }

    fn send_physical(&self, device: &str, control: Control, value: f32) {
        let ev = InputEvent {
            source: SourceId::Physical(device.into()),
            control,
            value,
            timestamp: std::time::Instant::now(),
        };
        self.events_tx.send(ev).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }

    fn last(&self) -> Option<XGamepad> {
        self.submitted.lock().unwrap().last().copied()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            drop(tx);
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[test]
fn ai_only_profile_press_south_appears_on_pad() {
    let h = Harness::new(FOUR_PROFILES_TOML);
    h.set_profile("ai-only");
    let agent = h.ai_handle("test-agent");
    agent.press(ButtonId::South, Duration::from_millis(200));
    std::thread::sleep(Duration::from_millis(30));
    let last = h.last().expect("at least one pad state submitted");
    assert_ne!(
        last.buttons.raw & vigem_client::XButtons::A,
        0,
        "South (A) button should be set"
    );
}

#[test]
fn fightstick_mixer_dpad_appears_as_right_stick() {
    let h = Harness::new(FOUR_PROFILES_TOML);
    // Default profile is fightstick-mixer.
    h.send_physical("fight_stick_2", Control::Button(ButtonId::DPadRight), 1.0);
    let last = h.last().expect("submitted");
    // RightStickX = +1.0 → thumb_rx near i16::MAX
    assert!(
        last.thumb_rx >= 32760,
        "expected near i16::MAX, got {}",
        last.thumb_rx
    );
}

#[test]
fn pad_passthrough_passes_button_through() {
    let h = Harness::new(FOUR_PROFILES_TOML);
    h.set_profile("pad-passthrough");
    h.send_physical("xbox_pad", Control::Button(ButtonId::East), 1.0);
    let last = h.last().expect("submitted");
    assert_ne!(
        last.buttons.raw & vigem_client::XButtons::B,
        0,
        "East (B) button should be set"
    );
}

#[test]
fn copilot_last_writer_wins_human_overrides_ai() {
    let h = Harness::new(FOUR_PROFILES_TOML);
    h.set_profile("fightstick-plus-ai");
    let agent = h.ai_handle("co_agent");
    agent.axis(AxisId::LeftStickX, 1.0);
    std::thread::sleep(Duration::from_millis(30));
    h.send_physical("fight_stick", Control::Axis(AxisId::LeftStickX), -1.0);
    let last = h.last().expect("submitted");
    assert!(
        last.thumb_lx <= -32760,
        "expected near i16::MIN (human override), got {}",
        last.thumb_lx
    );
}

#[test]
fn profile_switch_releases_dropped_devices() {
    let h = Harness::new(FOUR_PROFILES_TOML);
    h.set_profile("fightstick-plus-ai");
    let agent = h.ai_handle("agent");
    // Long-held press — will not auto-release during the test.
    agent.press(ButtonId::South, Duration::from_secs(60));
    std::thread::sleep(Duration::from_millis(50));

    let mid = h.last().expect("mid-state pad report");
    assert_ne!(
        mid.buttons.raw & vigem_client::XButtons::A,
        0,
        "AI press should appear on pad in copilot mode"
    );

    // Switch to fightstick-mixer (no AI input).
    h.set_profile("fightstick-mixer");
    let after = h.last().expect("post-switch pad report");
    // The held South should be absent because new profile doesn't consume AI.
    assert_eq!(
        after.buttons.raw & vigem_client::XButtons::A,
        0,
        "AI-held button should be cleared after switch to fightstick-mixer"
    );
}
