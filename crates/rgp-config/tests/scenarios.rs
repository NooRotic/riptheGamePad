use rgp_config::parse_str;

// ---------------------------------------------------------------------------
// Shared full-config TOML (all four scenarios in one file)
// ---------------------------------------------------------------------------

const ALL_SCENARIOS: &str = r#"
[devices]
fight_stick   = "uuid:03000000d62000002000000000007200"
fight_stick_2 = "uuid:03000000d62000002000000000007201"
xbox_pad      = "uuid:030000005e0400000202000000007200"

# Scenario 1: fight stick + 8-way camera stick
[[profile]]
id   = "fightstick-mixer"
name = "Fight Stick + Camera Stick"
inputs = ["fight_stick", "fight_stick_2"]

[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to   = "passthrough"

[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadUp" }
to   = { axis = "RightStickY", value = -1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadDown" }
to   = { axis = "RightStickY", value = 1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadLeft" }
to   = { axis = "RightStickX", value = -1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadRight" }
to   = { axis = "RightStickX", value = 1.0 }

# Scenario 2: standard pad passthrough
[[profile]]
id = "pad-passthrough"
name = "Standard Gamepad"
inputs = ["xbox_pad"]
[[profile.rule]]
from = { device = "xbox_pad", control = "*" }
to   = "passthrough"

# Scenario 3: AI-only
[[profile]]
id = "ai-only"
name = "AI Driver"
inputs = ["ai:*"]
[[profile.rule]]
from = { device = "ai:*", control = "*" }
to   = "passthrough"

# Scenario 4: human + AI co-pilot
[[profile]]
id = "fightstick-plus-ai"
name = "Fight Stick + AI Co-Pilot"
inputs = ["fight_stick", "ai:*"]
[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to   = "passthrough"
[[profile.rule]]
from = { device = "ai:*", control = "*" }
to   = "passthrough"

[default]
profile = "fightstick-mixer"

[server]
addr = "127.0.0.1:7777"

[hotkeys]
next_profile     = "F9"
prev_profile     = "F10"
panic_disconnect = "Ctrl+F12"
"#;

// ---------------------------------------------------------------------------
// Minimal fightstick-mixer-only TOML (matches task brief)
// ---------------------------------------------------------------------------

const FIGHTSTICK_MIXER: &str = r#"
[devices]
fight_stick   = "uuid:abc"
fight_stick_2 = "uuid:def"

[[profile]]
id = "fightstick-mixer"
name = "Fight Stick + Camera Stick"
inputs = ["fight_stick", "fight_stick_2"]

[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to = "passthrough"

[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadUp" }
to = { axis = "RightStickY", value = -1.0 }

[default]
profile = "fightstick-mixer"

[server]
addr = "127.0.0.1:7777"

[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

// ---------------------------------------------------------------------------
// Tests from the task brief
// ---------------------------------------------------------------------------

#[test]
fn fightstick_mixer_parses() {
    let cfg = parse_str(FIGHTSTICK_MIXER).expect("must parse");
    assert_eq!(cfg.profiles.len(), 1);
    assert_eq!(cfg.profiles[0].id.0, "fightstick-mixer");
    assert_eq!(cfg.default_profile().0, "fightstick-mixer");
}

#[test]
fn fightstick_mixer_compiles_to_lookup_table() {
    use rgp_core::{Control, ButtonId, AxisId};
    let cfg = parse_str(FIGHTSTICK_MIXER).unwrap();
    let compiled = cfg.compile(&"fightstick-mixer".into()).unwrap();
    let key = (
        rgp_core::DeviceMatcher::Exact("fight_stick_2".into()),
        Control::Button(ButtonId::DPadUp),
    );
    let action = compiled.rules.get(&key).expect("rule must exist");
    match action {
        rgp_config::RuleAction::SetAxis(AxisId::RightStickY, v) => assert_eq!(*v, -1.0),
        _ => panic!("wrong action: {action:?}"),
    }
}

#[test]
fn unknown_device_alias_in_inputs_is_validation_error() {
    let bad = r#"
[[profile]]
id = "p"
name = "p"
inputs = ["nonexistent_device"]
[[profile.rule]]
from = { device = "nonexistent_device", control = "*" }
to = "passthrough"
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    assert!(parse_str(bad).is_err());
}

#[test]
fn duplicate_profile_id_is_validation_error() {
    let bad = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P1"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "*" }
to = "passthrough"
[[profile]]
id = "p"
name = "P2"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "*" }
to = "passthrough"
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    assert!(parse_str(bad).is_err());
}

#[test]
fn unknown_control_name_in_rule_is_validation_error() {
    let bad = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "Nonsense" }
to = "passthrough"
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    assert!(parse_str(bad).is_err());
}

// ---------------------------------------------------------------------------
// Tests for the remaining three scenarios from spec §6
// ---------------------------------------------------------------------------

#[test]
fn pad_passthrough_parses() {
    let cfg = parse_str(ALL_SCENARIOS).expect("must parse");
    let profile = cfg.profiles.iter().find(|p| p.id.0 == "pad-passthrough")
        .expect("pad-passthrough profile must exist");
    assert_eq!(profile.name, "Standard Gamepad");
    assert_eq!(profile.inputs, vec!["xbox_pad"]);
}

#[test]
fn pad_passthrough_compiles_passthrough_device() {
    use rgp_core::DeviceMatcher;
    let cfg = parse_str(ALL_SCENARIOS).unwrap();
    let compiled = cfg.compile(&"pad-passthrough".into()).unwrap();
    assert!(compiled.passthrough.contains(&DeviceMatcher::Exact("xbox_pad".into())),
        "xbox_pad must be in passthrough set");
}

#[test]
fn ai_only_parses() {
    let cfg = parse_str(ALL_SCENARIOS).expect("must parse");
    let profile = cfg.profiles.iter().find(|p| p.id.0 == "ai-only")
        .expect("ai-only profile must exist");
    assert_eq!(profile.name, "AI Driver");
    assert_eq!(profile.inputs, vec!["ai:*"]);
}

#[test]
fn ai_only_compiles_passthrough_ai_any() {
    use rgp_core::DeviceMatcher;
    let cfg = parse_str(ALL_SCENARIOS).unwrap();
    let compiled = cfg.compile(&"ai-only".into()).unwrap();
    assert!(compiled.passthrough.contains(&DeviceMatcher::AiAny),
        "AiAny must be in passthrough set");
    assert!(compiled.inputs.contains(&DeviceMatcher::AiAny),
        "AiAny must be in inputs set");
}

#[test]
fn fightstick_plus_ai_parses() {
    let cfg = parse_str(ALL_SCENARIOS).expect("must parse");
    let profile = cfg.profiles.iter().find(|p| p.id.0 == "fightstick-plus-ai")
        .expect("fightstick-plus-ai profile must exist");
    assert_eq!(profile.name, "Fight Stick + AI Co-Pilot");
    assert_eq!(profile.inputs.len(), 2);
}

#[test]
fn fightstick_plus_ai_compiles_both_passthrough() {
    use rgp_core::DeviceMatcher;
    let cfg = parse_str(ALL_SCENARIOS).unwrap();
    let compiled = cfg.compile(&"fightstick-plus-ai".into()).unwrap();
    assert!(compiled.passthrough.contains(&DeviceMatcher::Exact("fight_stick".into())),
        "fight_stick must be in passthrough set");
    assert!(compiled.passthrough.contains(&DeviceMatcher::AiAny),
        "AiAny must be in passthrough set");
}

#[test]
fn all_scenarios_parse_and_compile() {
    let cfg = parse_str(ALL_SCENARIOS).expect("all scenarios must parse");
    assert_eq!(cfg.profiles.len(), 4);
    assert_eq!(cfg.default_profile().0, "fightstick-mixer");

    for profile in &cfg.profiles {
        cfg.compile(&profile.id).expect(&format!("compile({}) must succeed", profile.id.0));
    }
}

#[test]
fn fightstick_mixer_compiles_all_dpad_rules() {
    use rgp_core::{Control, ButtonId, AxisId, DeviceMatcher};
    let cfg = parse_str(ALL_SCENARIOS).unwrap();
    let compiled = cfg.compile(&"fightstick-mixer".into()).unwrap();

    let cases = [
        (ButtonId::DPadUp,    AxisId::RightStickY, -1.0_f32),
        (ButtonId::DPadDown,  AxisId::RightStickY,  1.0_f32),
        (ButtonId::DPadLeft,  AxisId::RightStickX, -1.0_f32),
        (ButtonId::DPadRight, AxisId::RightStickX,  1.0_f32),
    ];

    for (btn, expected_axis, expected_value) in cases {
        let key = (DeviceMatcher::Exact("fight_stick_2".into()), Control::Button(btn));
        let action = compiled.rules.get(&key)
            .unwrap_or_else(|| panic!("rule for {btn:?} must exist"));
        match action {
            RuleAction::SetAxis(axis, v) => {
                assert_eq!(*axis, expected_axis, "wrong axis for {btn:?}");
                assert_eq!(*v, expected_value, "wrong value for {btn:?}");
            }
            _ => panic!("wrong action for {btn:?}: {action:?}"),
        }
    }
}

use rgp_config::RuleAction;
