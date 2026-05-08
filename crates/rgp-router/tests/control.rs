use std::time::Duration;
use crossbeam_channel::bounded;
use rgp_core::*;
use rgp_config::parse_str;
use rgp_router::run as router_run;

const TOML: &str = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "*" }
to = "passthrough"
[default]
profile = "p"
[server]
addr = "127.0.0.1:7780"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

#[test]
fn panic_disconnect_zeros_state_and_emits_snapshot() {
    let cfg = parse_str(TOML).unwrap();
    let (events_tx, events_rx) = bounded(64);
    let (pad_tx, pad_rx) = bounded(64);
    let (ctl_tx, ctl_rx) = bounded(64);
    let (_sd_tx, sd_rx) = bounded::<()>(0);
    let _h = router_run(events_rx, ctl_rx, pad_tx, cfg, sd_rx);

    // Set a button via passthrough using the resolved device ID (not alias).
    events_tx.send(InputEvent {
        source: SourceId::Physical("uuid:1".into()),
        control: Control::Button(ButtonId::South),
        value: 1.0,
        timestamp: std::time::Instant::now(),
    }).unwrap();
    let s1 = pad_rx.recv_timeout(Duration::from_millis(200)).expect("first state");
    assert_eq!(s1.buttons.get(&ButtonId::South).copied(), Some(true));

    // Send panic disconnect.
    ctl_tx.send(ControlMsg::PanicDisconnect).unwrap();
    let s2 = pad_rx.recv_timeout(Duration::from_millis(200)).expect("panic state");
    assert!(s2.buttons.is_empty());
    assert!(s2.axes.is_empty());
}
