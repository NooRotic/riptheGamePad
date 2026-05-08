# Phase 2 Implementation Plan — Multi-Stick Mixer + RuleAction Modifiers

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the spec's primary "two physical fight sticks" mixer scenario actually work by adding XInput slot-based identification, and complete the v1-deferred `Mapping` modifier fields (`deadzone`, `invert`, `sensitivity`).

**Architecture:** Two layered, additive changes to v1's existing 9-crate workspace. (a) `SourceId::Physical(String)` carries `xinput:N` synthetic IDs for XInput devices instead of all-zero UUIDs; new `DeviceMatcher::XInputAny` matches them as a wildcard. (b) `Modifiers { deadzone, invert, sensitivity }` is built per rule from `Mapping`, stored alongside `RuleAction` in `CompiledProfile`, and applied to axis/trigger values in `apply_event`. No breaking changes to the v1 public API.

**Tech Stack:** Rust 2021, all v1 deps unchanged (`gilrs`, `vigem-client`, `crossbeam-channel`, `tokio`, `tray-icon`, `serde`, `toml`, `tracing`).

**Spec:** `docs/superpowers/specs/2026-05-08-phase2-design.md`. Read sections §3 (decisions), §4 (architecture), §5 (TOML examples), §6 (testing) before any task. The spec is the source of truth for *what*; this plan is the source of truth for *order and discipline*.

---

## File structure

```
crates/
├── rgp-core/
│   └── src/source.rs                     [modify] add DeviceMatcher::XInputAny + tests
├── rgp-input-physical/
│   └── src/lib.rs                        [modify] synthesize_source_id helper + use in list_connected/run
├── rgp-config/
│   ├── Cargo.toml                        [unchanged]
│   └── src/
│       ├── lib.rs                        [modify] validate updates: remove v1 rejections, add button-modifier rejection
│       ├── compile.rs                    [modify] CompiledProfile shape + xinput:* case + last-writer-wins warning
│       └── modifiers.rs                  [create] Modifiers struct + apply() + tests
├── rgp-router/
│   └── src/apply.rs                      [modify] honor modifiers in apply_action/apply_passthrough; update existing tests for new HashMap shape; +12 new modifier tests
└── rgp-app/
    ├── src/main.rs                       [unchanged]
    └── tests/integration.rs              [modify] +3 phase 2 tests using xinput:N + modifiers
assets/
└── config.default.toml                   [modify] mention xinput:N convention in comments
docs/superpowers/specs/
└── 2026-05-08-phase2-design.md           [unchanged]
```

---

## Task 1: `rgp-core` — `DeviceMatcher::XInputAny`

**Spec reference:** §4.1.

**Files:**
- Modify: `crates/rgp-core/src/source.rs`

- [ ] **Step 1: Add `XInputAny` variant and matcher arm**

Replace the existing `DeviceMatcher` enum and `impl` block in `crates/rgp-core/src/source.rs` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeviceMatcher {
    Exact(String),
    AiAny,
    AiClient(String),
    XInputAny,
}

impl DeviceMatcher {
    pub fn matches(&self, id: &SourceId) -> bool {
        match (self, id) {
            (DeviceMatcher::Exact(a), SourceId::Physical(b)) => a == b,
            (DeviceMatcher::AiAny, SourceId::Ai(_)) => true,
            (DeviceMatcher::AiClient(a), SourceId::Ai(b)) => a == b,
            (DeviceMatcher::XInputAny, SourceId::Physical(b)) => b.starts_with("xinput:"),
            _ => false,
        }
    }
}
```

- [ ] **Step 2: Add unit tests**

Append to the `#[cfg(test)] mod tests` block at the bottom of `crates/rgp-core/src/source.rs` (or the equivalent test module — currently `device_matcher_matches_correctly` lives in `event.rs`; add the new tests next to it):

```rust
#[test]
fn xinput_any_matches_xinput_slots() {
    use crate::source::DeviceMatcher;
    assert!(DeviceMatcher::XInputAny.matches(&SourceId::Physical("xinput:0".into())));
    assert!(DeviceMatcher::XInputAny.matches(&SourceId::Physical("xinput:1".into())));
    assert!(DeviceMatcher::XInputAny.matches(&SourceId::Physical("xinput:9".into())));
}

#[test]
fn xinput_any_rejects_non_xinput() {
    use crate::source::DeviceMatcher;
    assert!(!DeviceMatcher::XInputAny.matches(&SourceId::Physical("uuid:abc".into())));
    assert!(!DeviceMatcher::XInputAny.matches(&SourceId::Physical("xbox_pad".into())));
    assert!(!DeviceMatcher::XInputAny.matches(&SourceId::Ai("client".into())));
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p rgp-core
```
Expected: prior 2 tests + 2 new = 4 tests passing.

- [ ] **Step 4: Lint**

```bash
cargo clippy -p rgp-core -- -D warnings
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rgp-core/src/source.rs crates/rgp-core/src/event.rs
rtk git commit -m "feat(rgp-core): add DeviceMatcher::XInputAny for xinput:* wildcard"
```

(`event.rs` may need to land in the commit if you added tests there next to the existing matcher test. Adjust the `git add` to match where the tests landed.)

---

## Task 2: `rgp-input-physical` — `synthesize_source_id`

**Spec reference:** §4.3.

**Files:**
- Modify: `crates/rgp-input-physical/src/lib.rs`

This task is independent of Task 1 and could run in parallel.

- [ ] **Step 1: Add the pure helper at the top of `crates/rgp-input-physical/src/lib.rs`**

Add this function (before any `pub fn`s):

```rust
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
```

- [ ] **Step 2: Replace the inline UUID-formatting in `list_connected`**

Find the existing `list_connected` body (it currently does `format!("uuid:{}", uuid::Uuid::from_bytes(gp.uuid()))` inline). Replace with:

```rust
pub fn list_connected() -> Vec<DeviceInfo> {
    let mut gilrs = match Gilrs::new() {
        Ok(g) => g,
        Err(gilrs::Error::NotImplemented(g)) => g,
        Err(_) => return vec![],
    };
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
```

- [ ] **Step 3: Replace `source_id_for` in `run()`**

Find the helper (currently `fn source_id_for(gilrs: &Gilrs, id: GamepadId) -> String`) and replace with:

```rust
fn source_id_for(gilrs: &Gilrs, id: GamepadId) -> String {
    let slot = usize::from(id);
    match gilrs.connected_gamepad(id) {
        Some(gp) => synthesize_source_id(gp.uuid(), slot),
        None => format!("disconnected:{slot}"),
    }
}
```

- [ ] **Step 4: Add unit tests for the pure helper**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/rgp-input-physical/src/lib.rs` (or the existing `tests` module — there is none in `lib.rs` today; create it):

```rust
#[cfg(test)]
mod synthesize_tests {
    use super::synthesize_source_id;

    #[test]
    fn zero_uuid_returns_xinput_slot() {
        let id = synthesize_source_id([0u8; 16], 0);
        assert_eq!(id, "xinput:0");
        let id = synthesize_source_id([0u8; 16], 1);
        assert_eq!(id, "xinput:1");
        let id = synthesize_source_id([0u8; 16], 3);
        assert_eq!(id, "xinput:3");
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
        let with_slot_zero = synthesize_source_id(bytes, 0);
        let with_slot_seven = synthesize_source_id(bytes, 7);
        assert_eq!(with_slot_zero, with_slot_seven);
    }
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p rgp-input-physical
```
Expected: prior 8 tests + 3 new = 11 passing.

- [ ] **Step 6: Lint**

```bash
cargo clippy -p rgp-input-physical -- -D warnings
```
Expected: clean.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rgp-input-physical/src/lib.rs
rtk git commit -m "feat(rgp-input-physical): synthesize xinput:N source ids for XInput devices

XInput on Windows exposes slots, not stable per-device UUIDs; gilrs
returns all-zero UUIDs for any XInput device. Synthesize 'xinput:<slot>'
strings instead so two simultaneously-plugged XInput sticks can be
distinguished. Non-XInput devices keep their UUID-based identity."
```

---

## Task 3: `rgp-config` — `Modifiers` + `CompiledProfile` reshape + `xinput:*` wildcard + validate updates

**Spec reference:** §4.2, §3 decisions #4, #5, #6, #8, #10, §6.

This is the biggest task. Touches schema validation, compile-side data shape, and the `validate` rejections. After this task, `cargo build` will succeed but `rgp-router`'s tests WILL break because the `CompiledProfile.rules` value type changes from `RuleAction` to `(RuleAction, Modifiers)` and `passthrough` changes from `HashSet` to `HashMap`. Task 4 fixes the router. Build the workspace at the end of this task with `cargo build --workspace` to confirm `rgp-config` itself compiles; some sibling test suites will fail until Task 4.

**Files:**
- Create: `crates/rgp-config/src/modifiers.rs`
- Modify: `crates/rgp-config/src/lib.rs`
- Modify: `crates/rgp-config/src/compile.rs`
- Modify: `crates/rgp-config/tests/scenarios.rs`

- [ ] **Step 1: Create `crates/rgp-config/src/modifiers.rs`**

```rust
use crate::schema::Mapping;

#[derive(Debug, Clone, Copy)]
pub struct Modifiers {
    pub deadzone: f32,
    pub invert: bool,
    pub sensitivity: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers { deadzone: 0.0, invert: false, sensitivity: 1.0 }
    }
}

impl Modifiers {
    pub fn from_mapping(m: &Mapping) -> Self {
        Modifiers {
            deadzone: m.deadzone.unwrap_or(0.0),
            invert: m.invert,
            sensitivity: m.sensitivity.unwrap_or(1.0),
        }
    }

    pub fn is_default(&self) -> bool {
        self.deadzone == 0.0 && !self.invert && self.sensitivity == 1.0
    }

    /// Apply modifiers to an axis or trigger value.
    /// Threshold-style deadzone: values with |v| < deadzone are clamped to 0.
    /// Then sign-flip if invert, then multiply by sensitivity.
    pub fn apply(&self, mut v: f32) -> f32 {
        if v.abs() < self.deadzone {
            return 0.0;
        }
        if self.invert {
            v = -v;
        }
        v * self.sensitivity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_noop_on_axis_value() {
        let m = Modifiers::default();
        assert_eq!(m.apply(0.5), 0.5);
        assert_eq!(m.apply(-0.7), -0.7);
        assert_eq!(m.apply(1.0), 1.0);
        assert_eq!(m.apply(0.0), 0.0);
    }

    #[test]
    fn deadzone_zeroes_values_below_threshold() {
        let m = Modifiers { deadzone: 0.1, ..Modifiers::default() };
        assert_eq!(m.apply(0.05), 0.0);
        assert_eq!(m.apply(-0.05), 0.0);
        assert_eq!(m.apply(0.5), 0.5);
        assert_eq!(m.apply(-0.5), -0.5);
    }

    #[test]
    fn invert_flips_sign() {
        let m = Modifiers { invert: true, ..Modifiers::default() };
        assert_eq!(m.apply(0.5), -0.5);
        assert_eq!(m.apply(-0.7), 0.7);
        assert_eq!(m.apply(0.0), 0.0);
    }

    #[test]
    fn sensitivity_scales() {
        let m = Modifiers { sensitivity: 0.7, ..Modifiers::default() };
        assert!((m.apply(1.0) - 0.7).abs() < 1e-6);
        assert!((m.apply(-1.0) - -0.7).abs() < 1e-6);
    }

    #[test]
    fn combined_modifiers_compose() {
        let m = Modifiers { deadzone: 0.1, invert: true, sensitivity: 2.0 };
        // 0.05 below deadzone → 0
        assert_eq!(m.apply(0.05), 0.0);
        // 0.5 above deadzone, invert flips, sensitivity doubles → -1.0
        assert!((m.apply(0.5) - -1.0).abs() < 1e-6);
    }

    #[test]
    fn is_default_reports_correctly() {
        assert!(Modifiers::default().is_default());
        assert!(!Modifiers { deadzone: 0.1, ..Modifiers::default() }.is_default());
        assert!(!Modifiers { invert: true, ..Modifiers::default() }.is_default());
        assert!(!Modifiers { sensitivity: 0.5, ..Modifiers::default() }.is_default());
    }

    #[test]
    fn from_mapping_pulls_optional_fields() {
        use crate::schema::{Mapping, ControlSelector, RuleTarget};
        let m = Mapping {
            from: ControlSelector { device: "d".into(), control: "*".into() },
            to: RuleTarget::Passthrough("passthrough".into()),
            deadzone: Some(0.2),
            invert: true,
            sensitivity: Some(1.5),
        };
        let mods = Modifiers::from_mapping(&m);
        assert_eq!(mods.deadzone, 0.2);
        assert!(mods.invert);
        assert_eq!(mods.sensitivity, 1.5);
    }
}
```

- [ ] **Step 2: Add `pub mod modifiers;` to `crates/rgp-config/src/lib.rs`**

Find the existing `pub mod schema; pub mod compile;` block at the top of `crates/rgp-config/src/lib.rs` and add a third line:

```rust
pub mod schema;
pub mod compile;
pub mod modifiers;
```

Also add a re-export so callers can `use rgp_config::Modifiers`:

```rust
pub use schema::*;
pub use compile::{CompiledProfile, RuleAction};
pub use modifiers::Modifiers;
```

- [ ] **Step 3: Update `CompiledProfile` shape in `crates/rgp-config/src/compile.rs`**

Find the `pub struct CompiledProfile` and replace with:

```rust
use crate::modifiers::Modifiers;
use std::collections::{HashMap, HashSet};
use rgp_core::{ProfileId, DeviceMatcher, Control, ButtonId, AxisId, TriggerId, RgpError};

#[derive(Debug, Clone)]
pub struct CompiledProfile {
    pub id: ProfileId,
    pub inputs: HashSet<DeviceMatcher>,
    pub rules: HashMap<(DeviceMatcher, Control), (RuleAction, Modifiers)>,
    pub passthrough: HashMap<DeviceMatcher, Modifiers>,
}
```

- [ ] **Step 4: Update `Config::compile` in `crates/rgp-config/src/compile.rs`**

Replace the existing `impl Config { pub fn compile(...) ... }` block with:

```rust
impl super::schema::Config {
    pub fn compile(&self, id: &ProfileId) -> Result<CompiledProfile, RgpError> {
        let profile = self.profiles.iter().find(|p| &p.id == id)
            .ok_or_else(|| RgpError::Config { line: None,
                msg: format!("profile not found: {}", id.0) })?;
        let mut inputs = HashSet::new();
        let mut rules = HashMap::new();
        let mut passthrough: HashMap<DeviceMatcher, Modifiers> = HashMap::new();
        for input in &profile.inputs {
            inputs.insert(input_to_matcher(input));
        }
        for rule in &profile.rules {
            let dev = input_to_matcher(&rule.from.device);
            let modifiers = Modifiers::from_mapping(rule);
            match &rule.to {
                super::RuleTarget::Passthrough(s) if s == "passthrough" => {
                    if let Some(prev) = passthrough.insert(dev.clone(), modifiers) {
                        if !prev.is_default() && !modifiers.is_default() {
                            tracing::warn!(
                                target: "rgp::config",
                                device = ?dev,
                                "multiple passthrough rules with non-default modifiers; last wins"
                            );
                        }
                    }
                }
                super::RuleTarget::Passthrough(s) => {
                    return Err(RgpError::Config { line: None,
                        msg: format!("invalid 'to' string: {s}") });
                }
                super::RuleTarget::SetAxis { axis, value } => {
                    let from_ctl = parse_control(&rule.from.control)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    let to_axis = parse_axis(axis)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    rules.insert(
                        (dev, from_ctl),
                        (RuleAction::SetAxis(to_axis, *value), modifiers),
                    );
                }
                super::RuleTarget::SetButton { button, value } => {
                    let from_ctl = parse_control(&rule.from.control)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    let to_btn = parse_button(button)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    rules.insert(
                        (dev, from_ctl),
                        (RuleAction::SetButton(to_btn, *value), modifiers),
                    );
                }
            }
        }
        Ok(CompiledProfile { id: id.clone(), inputs, rules, passthrough })
    }
}
```

(The existing `tracing` crate is already a workspace dep — `rgp-config/Cargo.toml` may need `tracing = { workspace = true }` added if it's not there. Add if missing.)

- [ ] **Step 5: Add `xinput:*` recognition in `input_to_matcher`**

Find `input_to_matcher` in `compile.rs` and replace with:

```rust
pub(crate) fn input_to_matcher(s: &str) -> DeviceMatcher {
    if s == "ai:*" {
        DeviceMatcher::AiAny
    } else if let Some(id) = s.strip_prefix("ai:") {
        DeviceMatcher::AiClient(id.into())
    } else if s == "xinput:*" {
        DeviceMatcher::XInputAny
    } else {
        DeviceMatcher::Exact(s.into())
    }
}
```

(Strings like `"xinput:0"`, `"xinput:1"` fall through to `DeviceMatcher::Exact("xinput:0")`, etc., which is correct — only the wildcard `"xinput:*"` becomes `XInputAny`.)

- [ ] **Step 6: Update `validate` in `crates/rgp-config/src/lib.rs`**

Find the v1 validation block that rejects `deadzone`/`invert`/`sensitivity` (added in commit `ff3d920` — three `if r.deadzone.is_some()` / `if r.invert` / `if r.sensitivity.is_some()` blocks). Replace those three blocks with a single button-modifier rejection:

```rust
    // Reject modifiers on button-source rules. Modifiers (deadzone/invert/
    // sensitivity) are axis/trigger transformations and would either zero
    // out button presses (deadzone) or produce nonsense values.
    for p in &cfg.profiles {
        for r in &p.rules {
            let mods = crate::modifiers::Modifiers::from_mapping(r);
            if mods.is_default() {
                continue;
            }
            // The from.control is a string at this point; resolve it to a
            // Control to check if it's a button. Wildcard "*" allows modifiers
            // (they no-op for button events at runtime).
            if r.from.control == "*" {
                continue;
            }
            if let Ok(rgp_core::Control::Button(_)) = crate::compile::parse_control(&r.from.control) {
                return Err(RgpError::Config { line: None,
                    msg: format!(
                        "modifiers (deadzone/invert/sensitivity) cannot be applied to button rule (device={}, control={}); buttons are binary inputs",
                        r.from.device, r.from.control)
                });
            }
        }
    }
```

(Make sure the `parse_control` is accessible — it's `pub` in `compile.rs` already.)

- [ ] **Step 7: Update existing scenario tests if any rely on rejection of modifiers**

Three tests (`deadzone_field_rejected_in_v1`, `invert_field_rejected_in_v1`, `sensitivity_field_rejected_in_v1`) in `crates/rgp-config/tests/scenarios.rs` will now FAIL because their TOMLs put modifiers on `to = "passthrough"` rules with non-button source controls (e.g., `control = "South"` is a button — so they'd still be rejected by the new validation, BUT the assertion was on the v1 message containing "deadzone"/"invert"/"sensitivity").

Read each of those three tests; update each to:
- Use a button-source control (e.g., `control = "South"`) so the new validation triggers.
- Adjust the assertion to check the new message which mentions "modifiers" and "buttons are binary inputs".

For example, change `deadzone_field_rejected_in_v1` to:

```rust
#[test]
fn deadzone_on_button_rule_is_validation_error() {
    let bad = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "South" }
to = "passthrough"
deadzone = 0.1
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    let err = rgp_config::parse_str(bad).expect_err("must reject");
    let msg = format!("{err}");
    assert!(msg.contains("modifiers") || msg.contains("buttons"),
            "expected message about modifiers/buttons, got: {msg}");
}
```

Apply the same shape to `invert_on_button_rule_is_validation_error` and `sensitivity_on_button_rule_is_validation_error`.

- [ ] **Step 8: Add new scenario tests for modifiers on axes (now allowed)**

Append to `crates/rgp-config/tests/scenarios.rs`:

```rust
#[test]
fn deadzone_on_axis_rule_compiles() {
    let toml = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "LeftStickX" }
to = "passthrough"
deadzone = 0.1
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    let cfg = rgp_config::parse_str(toml).expect("must parse");
    let _compiled = cfg.compile(&"p".into()).expect("must compile");
}

#[test]
fn modifiers_on_wildcard_rule_compile() {
    let toml = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["d"]
[[profile.rule]]
from = { device = "d", control = "*" }
to = "passthrough"
deadzone = 0.05
sensitivity = 0.7
[default]
profile = "p"
[server]
addr = "127.0.0.1:7777"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;
    let cfg = rgp_config::parse_str(toml).expect("must parse");
    let compiled = cfg.compile(&"p".into()).expect("must compile");
    let dev = rgp_core::DeviceMatcher::Exact("d".into());
    let mods = compiled.passthrough.get(&dev).expect("device in passthrough");
    assert!((mods.deadzone - 0.05).abs() < 1e-6);
    assert!((mods.sensitivity - 0.7).abs() < 1e-6);
    assert!(!mods.invert);
}

#[test]
fn xinput_wildcard_compiles_to_XInputAny() {
    let toml = r#"
[devices]
d = "uuid:1"
[[profile]]
id = "p"
name = "P"
inputs = ["xinput:*"]
[[profile.rule]]
from = { device = "xinput:*", control = "*" }
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
    let cfg = rgp_config::parse_str(toml).expect("must parse");
    let compiled = cfg.compile(&"p".into()).expect("must compile");
    assert!(compiled.inputs.contains(&rgp_core::DeviceMatcher::XInputAny));
    assert!(compiled.passthrough.contains_key(&rgp_core::DeviceMatcher::XInputAny));
}
```

- [ ] **Step 9: Run `cargo test -p rgp-config`**

Expected: prior 14 tests (some renamed) + new tests = 14 + ~3 = 17+ passing in `rgp-config`. The 3 modified tests replace the 3 old ones; net gain is the three modifier-allowed tests.

`cargo test --workspace` will now FAIL because `rgp-router::apply` tests reference the old `CompiledProfile` shape. That's expected — Task 4 fixes them.

- [ ] **Step 10: Lint**

```bash
cargo clippy -p rgp-config -- -D warnings
```
Expected: clean (workspace-wide clippy may still fail until Task 4).

- [ ] **Step 11: Commit**

```bash
rtk git add crates/rgp-config
rtk git commit -m "feat(rgp-config): add Modifiers struct, embed in CompiledProfile

- New crates/rgp-config/src/modifiers.rs with Modifiers + Modifiers::apply
- CompiledProfile.rules value: RuleAction → (RuleAction, Modifiers)
- CompiledProfile.passthrough: HashSet<DeviceMatcher> → HashMap<DeviceMatcher, Modifiers>
- input_to_matcher recognizes 'xinput:*' wildcard
- validate: replace v1 rejection of modifiers with a button-source-only rejection
- Tracing warning on conflicting passthrough modifiers (last-writer-wins)

NOTE: rgp-router tests will break against the new shape; Task 4 fixes them."
```

---

## Task 4: `rgp-router` — `apply_event` honors modifiers

**Spec reference:** §4.4, §6 (12 router tests).

**Files:**
- Modify: `crates/rgp-router/src/apply.rs`

This task fixes the workspace build. Existing router tests that construct `CompiledProfile` manually use:
- `rules.insert((matcher, control), RuleAction::SetAxis(...))` — must become `rules.insert((matcher, control), (RuleAction::SetAxis(...), Modifiers::default()))`.
- `passthrough.insert(matcher)` (HashSet) — must become `passthrough.insert(matcher, Modifiers::default())` (HashMap).

- [ ] **Step 1: Update imports at the top of `crates/rgp-router/src/apply.rs`**

```rust
use rgp_core::{InputEvent, PadState, Control, DeviceMatcher};
use rgp_config::{CompiledProfile, RuleAction, Modifiers};
```

- [ ] **Step 2: Update `apply_event` to look up the (action, modifiers) tuple**

```rust
pub fn apply_event(state: &mut PadState, profile: &CompiledProfile, event: &InputEvent) -> bool {
    let matchers: Vec<&DeviceMatcher> = profile.inputs.iter()
        .filter(|m| m.matches(&event.source))
        .collect();
    if matchers.is_empty() { return false; }

    for m in &matchers {
        let key = (DeviceMatcher::clone(m), event.control);
        if let Some((action, modifiers)) = profile.rules.get(&key) {
            return apply_action(state, action, modifiers, event);
        }
    }

    if let Some(modifiers) = matchers.iter()
        .find_map(|m| profile.passthrough.get(*m))
    {
        return apply_passthrough(state, modifiers, event);
    }

    false
}
```

- [ ] **Step 3: Update `apply_action` and `apply_passthrough` to apply modifiers**

```rust
fn apply_action(
    state: &mut PadState,
    action: &RuleAction,
    modifiers: &Modifiers,
    event: &InputEvent,
) -> bool {
    match action {
        RuleAction::SetButton(b, target_when_pressed) => {
            // Modifiers no-op on button targets.
            let pressed = event.value > 0.5;
            let new_val = if pressed { *target_when_pressed } else { !*target_when_pressed };
            let prev = state.buttons.insert(*b, new_val);
            prev != Some(new_val)
        }
        RuleAction::SetAxis(a, magnitude) => {
            let new_val = match event.control {
                Control::Button(_) => {
                    // Button-driven SetAxis: modifiers no-op (binary input).
                    if event.value > 0.5 { *magnitude } else { 0.0 }
                }
                Control::Axis(_) => {
                    // Apply modifiers to the source axis value, then preserve sign convention.
                    let modulated = modifiers.apply(event.value);
                    modulated * magnitude.signum()
                }
                Control::Trigger(_) => {
                    // Trigger drives axis: apply modifiers, then scale by magnitude.
                    let modulated = modifiers.apply(event.value);
                    *magnitude * modulated
                }
            };
            let prev = state.axes.insert(*a, new_val);
            match prev {
                Some(p) => (p - new_val).abs() > f32::EPSILON,
                None => new_val != 0.0,
            }
        }
        RuleAction::PassControlSameName => apply_passthrough(state, modifiers, event),
        RuleAction::Drop => false,
    }
}

fn apply_passthrough(
    state: &mut PadState,
    modifiers: &Modifiers,
    event: &InputEvent,
) -> bool {
    match event.control {
        Control::Button(b) => {
            // Modifiers no-op on buttons. event.value is 0.0 or 1.0.
            let pressed = event.value > 0.5;
            state.buttons.insert(b, pressed) != Some(pressed)
        }
        Control::Axis(a) => {
            let new_val = modifiers.apply(event.value);
            let prev = state.axes.insert(a, new_val);
            match prev {
                Some(p) => (p - new_val).abs() > f32::EPSILON,
                None => new_val != 0.0,
            }
        }
        Control::Trigger(t) => {
            let new_val = modifiers.apply(event.value);
            let prev = state.triggers.insert(t, new_val);
            match prev {
                Some(p) => (p - new_val).abs() > f32::EPSILON,
                None => new_val != 0.0,
            }
        }
    }
}
```

- [ ] **Step 4: Update existing tests in `apply.rs` to use the new tuple/map shape**

Find each existing test that builds a `CompiledProfile` manually. Update:

1. `rules.insert((matcher, ctrl), action)` → `rules.insert((matcher, ctrl), (action, Modifiers::default()))`
2. `let mut passthrough = HashSet::new(); passthrough.insert(matcher);` → `let mut passthrough = HashMap::new(); passthrough.insert(matcher, Modifiers::default());`
3. `profile.passthrough.contains(&matcher)` → `profile.passthrough.contains_key(&matcher)` if any tests use it

There are ~6 helper functions that build profiles (`fightstick_mixer_profile`, `ai_only_profile`, `copilot_profile`, etc.). Update each so the rest of the tests compile against the new shape. Keep a `use rgp_config::Modifiers;` at the top of the test module.

- [ ] **Step 5: Run existing tests, confirm they still pass under new shape**

```bash
cargo test -p rgp-router
```
Expected: prior 40 tests pass against the updated shape, no behavior change yet (modifiers are all default = no-op).

- [ ] **Step 6: Add modifier behavior tests**

Append these 12 tests to the `#[cfg(test)] mod tests` block in `apply.rs`:

```rust
fn passthrough_profile_with_modifiers(dev_alias: &str, mods: Modifiers) -> CompiledProfile {
    let dev = DeviceMatcher::Exact(dev_alias.into());
    let mut inputs = HashSet::new();
    inputs.insert(dev.clone());
    let mut passthrough = HashMap::new();
    passthrough.insert(dev, mods);
    CompiledProfile {
        id: ProfileId("test".into()),
        inputs,
        rules: HashMap::new(),
        passthrough,
    }
}

#[test]
fn deadzone_clamps_small_axis_to_zero() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { deadzone: 0.1, ..Modifiers::default() });
    let mut state = PadState::default();
    let e = ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 0.05);
    let changed = apply_event(&mut state, &profile, &e);
    // 0.05 below 0.1 deadzone → 0.0; with no prior entry, no change.
    assert!(!changed);
    // Entry is inserted at 0.0; verify.
    assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap_or(&999.0), 0.0);
}

#[test]
fn deadzone_does_not_affect_above_threshold() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { deadzone: 0.1, ..Modifiers::default() });
    let mut state = PadState::default();
    let e = ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 0.5);
    apply_event(&mut state, &profile, &e);
    assert!((state.axes.get(&AxisId::LeftStickX).unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn invert_flips_axis_sign_through_passthrough() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { invert: true, ..Modifiers::default() });
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::RightStickY), 0.7));
    assert!((state.axes.get(&AxisId::RightStickY).unwrap() - -0.7).abs() < 1e-6);
}

#[test]
fn sensitivity_scales_axis_linearly() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { sensitivity: 0.5, ..Modifiers::default() });
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 1.0));
    assert!((state.axes.get(&AxisId::LeftStickX).unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn modifiers_combine_deadzone_invert_sensitivity() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { deadzone: 0.1, invert: true, sensitivity: 2.0 });
    let mut state = PadState::default();
    // 0.05 < 0.1 → 0
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 0.05));
    assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap(), 0.0);
    // 0.5 above deadzone, invert flips, sensitivity 2x → -1.0
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::RightStickX), 0.5));
    assert!((state.axes.get(&AxisId::RightStickX).unwrap() - -1.0).abs() < 1e-6);
}

#[test]
fn modifiers_apply_to_trigger_through_passthrough() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { sensitivity: 0.5, ..Modifiers::default() });
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Trigger(TriggerId::R2), 1.0));
    assert!((state.triggers.get(&TriggerId::R2).unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn modifiers_noop_on_button_passthrough() {
    let profile = passthrough_profile_with_modifiers("d",
        Modifiers { deadzone: 0.5, invert: true, sensitivity: 0.5 });
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Button(ButtonId::South), 1.0));
    // Buttons unaffected by modifiers — pressed should be true.
    assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
}

#[test]
fn set_axis_from_axis_applies_modifiers() {
    let mut rules = HashMap::new();
    let dev = DeviceMatcher::Exact("d".into());
    rules.insert(
        (dev.clone(), Control::Axis(AxisId::LeftStickX)),
        (RuleAction::SetAxis(AxisId::RightStickX, 1.0),
         Modifiers { sensitivity: 0.5, ..Modifiers::default() }),
    );
    let mut inputs = HashSet::new();
    inputs.insert(dev);
    let profile = CompiledProfile {
        id: ProfileId("p".into()),
        inputs, rules, passthrough: HashMap::new(),
    };
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 1.0));
    // Sensitivity 0.5 applied first → 0.5; magnitude.signum() = 1.0 → 0.5
    assert!((state.axes.get(&AxisId::RightStickX).unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn set_axis_from_button_ignores_modifiers() {
    let mut rules = HashMap::new();
    let dev = DeviceMatcher::Exact("d".into());
    rules.insert(
        (dev.clone(), Control::Button(ButtonId::DPadRight)),
        (RuleAction::SetAxis(AxisId::RightStickX, 1.0),
         // Modifiers are present but should no-op on button source.
         Modifiers { sensitivity: 0.5, ..Modifiers::default() }),
    );
    let mut inputs = HashSet::new();
    inputs.insert(dev);
    let profile = CompiledProfile {
        id: ProfileId("p".into()),
        inputs, rules, passthrough: HashMap::new(),
    };
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("d".into()), Control::Button(ButtonId::DPadRight), 1.0));
    // Button-driven SetAxis ignores modifiers; result is magnitude (1.0).
    assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 1.0);
}

#[test]
fn xinput_any_matches_slot_zero_through_passthrough() {
    let dev = DeviceMatcher::XInputAny;
    let mut inputs = HashSet::new();
    inputs.insert(dev.clone());
    let mut passthrough = HashMap::new();
    passthrough.insert(dev, Modifiers::default());
    let profile = CompiledProfile {
        id: ProfileId("p".into()),
        inputs, rules: HashMap::new(), passthrough,
    };
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("xinput:0".into()), Control::Button(ButtonId::South), 1.0));
    assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
}

#[test]
fn xinput_any_matches_slot_one_distinct_from_slot_zero() {
    // Two distinct sources matching the same wildcard write to the same pad.
    let dev = DeviceMatcher::XInputAny;
    let mut inputs = HashSet::new();
    inputs.insert(dev.clone());
    let mut passthrough = HashMap::new();
    passthrough.insert(dev, Modifiers::default());
    let profile = CompiledProfile {
        id: ProfileId("p".into()),
        inputs, rules: HashMap::new(), passthrough,
    };
    let mut state = PadState::default();
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("xinput:0".into()), Control::Button(ButtonId::South), 1.0));
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("xinput:1".into()), Control::Button(ButtonId::East), 1.0));
    assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(true));
}

#[test]
fn exact_xinput_slot_matches_only_that_slot() {
    let dev = DeviceMatcher::Exact("xinput:0".into());
    let mut inputs = HashSet::new();
    inputs.insert(dev.clone());
    let mut passthrough = HashMap::new();
    passthrough.insert(dev, Modifiers::default());
    let profile = CompiledProfile {
        id: ProfileId("p".into()),
        inputs, rules: HashMap::new(), passthrough,
    };
    let mut state = PadState::default();
    // slot 0 matches
    apply_event(&mut state, &profile,
        &ev(SourceId::Physical("xinput:0".into()), Control::Button(ButtonId::South), 1.0));
    assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    // slot 1 does NOT match Exact("xinput:0")
    let changed = apply_event(&mut state, &profile,
        &ev(SourceId::Physical("xinput:1".into()), Control::Button(ButtonId::East), 1.0));
    assert!(!changed);
}
```

- [ ] **Step 7: Run all router tests**

```bash
cargo test -p rgp-router
```
Expected: prior 40 + 12 new = 52 passing.

- [ ] **Step 8: Run workspace tests**

```bash
cargo test --workspace
```
Expected: workspace-wide green again.

- [ ] **Step 9: Lint**

```bash
cargo clippy --workspace -- -D warnings
```
Expected: clean.

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rgp-router/src/apply.rs
rtk git commit -m "feat(rgp-router): apply Modifiers in apply_event value pipeline

- apply_event looks up (RuleAction, Modifiers) tuple from rules
- apply_passthrough takes Modifiers param, applies to axis/trigger values
- apply_action: SetAxis from axis source applies modifiers; SetAxis from
  button source ignores them; SetButton ignores them
- Modifiers no-op on button events through passthrough (binary inputs)
- 12 new tests covering each modifier dimension and XInput wildcard"
```

---

## Task 5: `rgp-app` — integration tests + config example update

**Spec reference:** §5 (TOML examples), §6 (3 integration tests).

**Files:**
- Modify: `crates/rgp-app/tests/integration.rs`
- Modify: `assets/config.default.toml`

- [ ] **Step 1: Add 3 integration tests to `crates/rgp-app/tests/integration.rs`**

Append at the bottom of the file:

```rust
const TWO_STICK_MIXER_TOML: &str = r#"
[devices]
fight_stick   = "xinput:0"
fight_stick_2 = "xinput:1"

[[profile]]
id = "two-stick-mixer"
name = "Two Stick Mixer"
inputs = ["fight_stick", "fight_stick_2"]
[[profile.rule]]
from = { device = "fight_stick", control = "*" }
to = "passthrough"
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadRight" }
to = { axis = "RightStickX", value = 1.0 }
[[profile.rule]]
from = { device = "fight_stick_2", control = "DPadLeft" }
to = { axis = "RightStickX", value = -1.0 }

[default]
profile = "two-stick-mixer"
[server]
addr = "127.0.0.1:7780"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

#[test]
fn two_stick_mixer_with_xinput_slot_aliases() {
    let h = Harness::new(TWO_STICK_MIXER_TOML);
    // Stick 1 (xinput:0) sends a face button press — should pass through.
    h.send_physical("xinput:0", Control::Button(ButtonId::South), 1.0);
    // Stick 2 (xinput:1) sends DPadRight — should set RightStickX.
    h.send_physical("xinput:1", Control::Button(ButtonId::DPadRight), 1.0);

    let last = h.last().expect("submitted");
    assert_ne!(last.buttons.raw & vigem_client::XButtons::A, 0,
               "xinput:0 South should set A bit");
    assert!(last.thumb_rx >= 32760,
            "xinput:1 DPadRight should set RightStickX to max, got {}", last.thumb_rx);
}

const DEADZONE_TOML: &str = r#"
[devices]
pad = "xinput:0"

[[profile]]
id = "deadzone-test"
name = "Deadzone"
inputs = ["pad"]
[[profile.rule]]
from = { device = "pad", control = "*" }
to = "passthrough"
deadzone = 0.2

[default]
profile = "deadzone-test"
[server]
addr = "127.0.0.1:7781"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

#[test]
fn deadzone_applied_through_full_pipeline() {
    let h = Harness::new(DEADZONE_TOML);
    // Below deadzone — should be zeroed.
    h.send_physical("xinput:0", Control::Axis(AxisId::LeftStickX), 0.1);
    let mid = h.last().expect("mid");
    assert_eq!(mid.thumb_lx, 0, "0.1 below 0.2 deadzone should be zero, got {}", mid.thumb_lx);
    // Above deadzone — should pass through.
    h.send_physical("xinput:0", Control::Axis(AxisId::LeftStickX), 0.5);
    let after = h.last().expect("after");
    assert!(after.thumb_lx > 16000, "0.5 above deadzone should pass; got {}", after.thumb_lx);
}

const INVERT_Y_TOML: &str = r#"
[devices]
pad = "xinput:0"

[[profile]]
id = "invert-y"
name = "Invert Y"
inputs = ["pad"]
[[profile.rule]]
from = { device = "pad", control = "RightStickY" }
to = "passthrough"
invert = true

[default]
profile = "invert-y"
[server]
addr = "127.0.0.1:7782"
[hotkeys]
next_profile = "F9"
prev_profile = "F10"
panic_disconnect = "Ctrl+F12"
"#;

#[test]
fn inverted_axis_visible_on_virtual_pad() {
    let h = Harness::new(INVERT_Y_TOML);
    // Push up (+1.0) → should appear as down (-1.0 on virtual pad).
    h.send_physical("xinput:0", Control::Axis(AxisId::RightStickY), 1.0);
    let last = h.last().expect("submitted");
    assert!(last.thumb_ry <= -32760,
            "inverted +1.0 should appear as i16::MIN-ish, got {}", last.thumb_ry);
}
```

- [ ] **Step 2: Update `assets/config.default.toml` to mention `xinput:N`**

Find the `[devices]` section in `assets/config.default.toml` (currently has commented-out UUID examples) and replace with:

```toml
[devices]
# Aliases here once you know your device's identifier. For XInput devices
# (Xbox 360 / Xbox One controllers, sticks in X-Input mode) use 'xinput:0',
# 'xinput:1', etc. — the slot number maps to gilrs's GamepadId order.
# For non-XInput devices (rare on Windows), use 'uuid:...' as printed by
# `riptheGamePad --list-devices`.
#
# fight_stick   = "xinput:0"
# fight_stick_2 = "xinput:1"
# xbox_pad      = "xinput:0"
```

- [ ] **Step 3: Run integration tests**

```bash
cargo test -p rgp-app --test integration
```
Expected: prior 6 + 3 new = 9 tests passing.

- [ ] **Step 4: Run full workspace tests**

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
Expected: 104 (v1 baseline) + ~25 new (Tasks 1–4 added 2+3+8+12 = 25) = 129+ passing. Clippy clean.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rgp-app/tests/integration.rs assets/config.default.toml
rtk git commit -m "feat(rgp-app): integration tests for two-stick mixer + modifiers

Three integration tests exercising the new phase 2 features end-to-end:
- Two-stick mixer using xinput:0 and xinput:1 aliases
- Deadzone applied through the full event → router → pad pipeline
- Inverted axis produces correct sign on the virtual pad

Also updates assets/config.default.toml to document the xinput:N
identifier convention surfaced by --list-devices."
```

---

## Self-Review

**1. Spec coverage:**

- §3 decision #1 (xinput:N strings): Task 2.
- §3 decision #2 (XInputAny matcher): Task 1.
- §3 decision #3 (Modifiers in CompiledProfile, applied in apply_event): Tasks 3, 4.
- §3 decision #4 (passthrough HashMap): Task 3 step 3.
- §3 decision #5 (rules tuple): Task 3 step 3.
- §3 decision #6 (button-modifier rejection at config-load): Task 3 step 6.
- §3 decision #7 (modifiers no-op on button events at runtime): Task 4 steps 2–3.
- §3 decision #8 (last-writer-wins with tracing warn): Task 3 step 4.
- §3 decision #9 (backward compat): no explicit task; verified via existing tests still passing.
- §3 decision #10 (default Modifiers no-op): Task 3 step 1.
- §4.1 (DeviceMatcher::XInputAny): Task 1.
- §4.2 (Modifiers + CompiledProfile + validate + compile): Task 3.
- §4.3 (synthesize_source_id): Task 2.
- §4.4 (apply_event modifier pipeline): Task 4.
- §5 (TOML examples): Task 5 step 1 (test TOMLs) and step 2 (config.default.toml).
- §6 testing target: 2 (rgp-core) + 3 (rgp-input-physical) + ~3 modifier scenario tests + 3 v1-rejection rewrites (rgp-config) + 12 (rgp-router) + 3 (rgp-app) = ~26 new tests. Spec target was ~25. Close.

**2. Placeholder scan:** No "TBD"/"TODO" in the plan; all step instructions have explicit code or commands.

**3. Type consistency:** `Modifiers` is consistently `pub struct Modifiers` with `deadzone: f32`, `invert: bool`, `sensitivity: f32`. `CompiledProfile.rules` is consistently `HashMap<(DeviceMatcher, Control), (RuleAction, Modifiers)>`. `passthrough` is consistently `HashMap<DeviceMatcher, Modifiers>` (HashSet → HashMap is a deliberate Task 3 change, propagated through Tasks 4 and 5).

**Forward-references requiring care:**
- Task 3 deliberately leaves the workspace in a broken state (router tests fail to compile against the new shape). Task 4's first step is to fix that. A subagent executing Task 3 in isolation should NOT push to a shared branch until Task 4 also lands.
- `tracing` may need to be added to `rgp-config/Cargo.toml` if not already there (Task 3 step 4). Subagent should check and add.
- Test count assertions in this plan (e.g., "prior 14 tests" in `rgp-config`) are approximate; the actual count depends on exact existing test counts, which the subagent should verify with `cargo test -p rgp-config 2>&1 | grep "test result"` rather than relying on the plan's number.
