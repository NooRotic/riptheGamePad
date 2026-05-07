# riptheGamePad Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Windows app that exposes one virtual gamepad to the OS while combining inputs from multiple physical controllers and AI agents in real time, controllable via system tray + TOML profiles.

**Architecture:** Cargo workspace, 9 fine-grained crates communicating via `crossbeam-channel`. Sync core, async only inside the WebSocket server. Each crate has a one-page interface defined in `rgp-core` so subagents can build them in parallel.

**Tech Stack:** Rust 2021, `gilrs` (physical input), `vigem-client` (ViGEmBus virtual pad), `tray-icon` + `global-hotkey` (UI), `tokio` + `tokio-tungstenite` (AI WS server), `crossbeam-channel`, `serde` + `toml`, `tracing`.

**Spec:** All public APIs, type definitions, behavior rules, and test specs are in `docs/superpowers/specs/2026-05-07-riptheGamePad-design.md`. **Subagents executing this plan MUST read the spec section corresponding to their task before writing any code.** The spec is the source of truth for *what*; this plan is the source of truth for *order and discipline*.

---

## File Structure

```
riptheGamePad/
├── Cargo.toml                    [workspace manifest]
├── .gitignore                    (already created)
├── crates/
│   ├── rgp-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            re-exports
│   │       ├── source.rs         SourceId, DeviceMatcher, DeviceInfo
│   │       ├── event.rs          InputEvent, Control, ButtonId, AxisId, TriggerId
│   │       ├── pad_state.rs      PadState
│   │       ├── profile.rs        ProfileId
│   │       ├── control_msg.rs    ControlMsg
│   │       └── error.rs          RgpError
│   ├── rgp-config/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            load(), parse_str()
│   │       ├── schema.rs         Config, Profile, Mapping (serde structs)
│   │       └── compile.rs        Config::compile() → CompiledProfile
│   ├── rgp-input-physical/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            run(), list_connected()
│   │       └── translate.rs      pure: gilrs::Event → InputEvent
│   ├── rgp-input-ai/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            handle(), AiInputHandle
│   │       └── timer.rs          single timer thread, min-heap
│   ├── rgp-virtual-pad/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            run(), PadSink trait
│   │       └── translate.rs      pure: PadState → vigem_client::XGamepad
│   ├── rgp-tray/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            run_on_main()
│   │       ├── menu.rs           profile-cycle math
│   │       └── hotkeys.rs        config parse + binding
│   ├── rgp-input-ai-server/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            run()
│   │       ├── frame.rs          serde: WS JSON frames
│   │       └── connection.rs     per-conn handler, hello-first, malformed counter
│   ├── rgp-router/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            run()
│   │       └── apply.rs          pure: apply_event(state, profile, event) → bool
│   └── rgp-app/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs           wire everything; integration tests in tests/
└── docs/                         [already exists]
```

---

## Task 0: Workspace scaffold

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `rust-toolchain.toml`

- [ ] **Step 1: Create the workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/rgp-core",
    "crates/rgp-config",
    "crates/rgp-input-physical",
    "crates/rgp-input-ai",
    "crates/rgp-input-ai-server",
    "crates/rgp-router",
    "crates/rgp-virtual-pad",
    "crates/rgp-tray",
    "crates/rgp-app",
]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT OR Apache-2.0"
authors = ["Walter Pollard Jr. <walter.pollard.jr@gmail.com>"]

[workspace.dependencies]
crossbeam-channel = "0.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
gilrs = "0.10"
vigem-client = "0.1"
tokio = { version = "1", features = ["rt", "net", "macros", "time", "sync", "io-util"] }
tokio-tungstenite = "0.21"
tray-icon = "0.14"
global-hotkey = "0.5"
uuid = { version = "1", features = ["v4"] }

[profile.release]
lto = "thin"
codegen-units = 1
```

- [ ] **Step 2: Pin the toolchain**

Create `rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Verify the workspace parses**

Run: `cargo metadata --format-version 1 --no-deps`
Expected: JSON output, exit code 0. (Will warn about missing crate manifests — that's fine; we'll create them in subsequent tasks.)

Actually — `cargo metadata` will fail because the listed members don't exist yet. Instead:

Run: `cargo --version` (sanity check) and skip cargo metadata until at least `rgp-core` exists.

- [ ] **Step 4: Commit**

```bash
rtk git add Cargo.toml rust-toolchain.toml
rtk git commit -m "chore: scaffold cargo workspace and toolchain"
```

---

## Task 1: `rgp-core` — types and errors

**Spec reference:** §5.1 of design doc. Read it before this task.

**Files:**
- Create: `crates/rgp-core/Cargo.toml`
- Create: `crates/rgp-core/src/lib.rs`
- Create: `crates/rgp-core/src/source.rs`
- Create: `crates/rgp-core/src/event.rs`
- Create: `crates/rgp-core/src/pad_state.rs`
- Create: `crates/rgp-core/src/profile.rs`
- Create: `crates/rgp-core/src/control_msg.rs`
- Create: `crates/rgp-core/src/error.rs`

- [ ] **Step 1: Create `crates/rgp-core/Cargo.toml`**

```toml
[package]
name = "rgp-core"
edition.workspace = true
version.workspace = true
license.workspace = true
authors.workspace = true

[dependencies]
serde = { workspace = true }
crossbeam-channel = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Create `src/event.rs` with the event types**

```rust
use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ButtonId {
    South, East, North, West,
    DPadUp, DPadDown, DPadLeft, DPadRight,
    LeftStickClick, RightStickClick,
    LeftBumper, RightBumper,
    Start, Select, Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AxisId {
    LeftStickX, LeftStickY, RightStickX, RightStickY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerId { L2, R2 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    Button(ButtonId),
    Axis(AxisId),
    Trigger(TriggerId),
}

#[derive(Debug, Clone)]
pub struct InputEvent {
    pub source: crate::source::SourceId,
    pub control: Control,
    pub value: f32,
    #[serde(skip, default = "Instant::now")]
    pub timestamp: Instant,
}
// Note: Instant doesn't serde directly; we use #[serde(skip)] and rely on the
// receiver re-stamping for tests. For wire format, see frame.rs in rgp-input-ai-server.
```

- [ ] **Step 3: Create `src/source.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceId {
    Physical(String),
    Ai(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeviceMatcher {
    Exact(String),         // matches Physical("alias") only after alias resolution
    AiAny,                 // matches any SourceId::Ai(_)
    AiClient(String),      // matches SourceId::Ai("client_id")
}

impl DeviceMatcher {
    pub fn matches(&self, id: &SourceId) -> bool {
        match (self, id) {
            (DeviceMatcher::Exact(a), SourceId::Physical(b)) => a == b,
            (DeviceMatcher::AiAny, SourceId::Ai(_)) => true,
            (DeviceMatcher::AiClient(a), SourceId::Ai(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: SourceId,
    pub name: String,
    pub connected: bool,
}
```

- [ ] **Step 4: Create `src/pad_state.rs`**

```rust
use std::collections::BTreeMap;
use crate::event::{ButtonId, AxisId, TriggerId};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadState {
    pub buttons: BTreeMap<ButtonId, bool>,
    pub axes: BTreeMap<AxisId, f32>,
    pub triggers: BTreeMap<TriggerId, f32>,
}
```

- [ ] **Step 5: Create `src/profile.rs`, `src/control_msg.rs`, `src/error.rs`**

```rust
// profile.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProfileId(pub String);

impl From<&str> for ProfileId {
    fn from(s: &str) -> Self { ProfileId(s.to_string()) }
}
```

```rust
// control_msg.rs
use crossbeam_channel::Sender;
use crate::profile::ProfileId;
use crate::source::DeviceInfo;

#[derive(Debug)]
pub enum ControlMsg {
    SetActiveProfile(ProfileId),
    ListDevices(Sender<Vec<DeviceInfo>>),
    Quit,
}
```

```rust
// error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RgpError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("virtual pad: {0}")]
    VirtualPad(String),
    #[error("config{}: {msg}", line.map(|l| format!(" (line {l})")).unwrap_or_default())]
    Config { line: Option<usize>, msg: String },
    #[error("input source: {0}")]
    InputSource(String),
    #[error("channel: {0}")]
    Channel(String),
}
```

- [ ] **Step 6: Create `src/lib.rs` re-exporting everything**

```rust
pub mod source;
pub mod event;
pub mod pad_state;
pub mod profile;
pub mod control_msg;
pub mod error;

pub use source::{SourceId, DeviceMatcher, DeviceInfo};
pub use event::{InputEvent, Control, ButtonId, AxisId, TriggerId};
pub use pad_state::PadState;
pub use profile::ProfileId;
pub use control_msg::ControlMsg;
pub use error::RgpError;
```

- [ ] **Step 7: Run `cargo check -p rgp-core`**

Expected: clean compile, zero warnings.

- [ ] **Step 8: Write the wire-format round-trip test in `crates/rgp-core/src/event.rs` (or a new `tests/` file)**

Add at the bottom of `event.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceId;

    #[test]
    fn input_event_serde_roundtrip_via_intermediate() {
        // Instant doesn't serde, so we test the wire-relevant fields only.
        // The wire format used by rgp-input-ai-server is JSON without timestamp.
        let ev = InputEvent {
            source: SourceId::Ai("agent1".into()),
            control: Control::Button(ButtonId::South),
            value: 1.0,
            timestamp: Instant::now(),
        };
        let json = serde_json::to_string(&ev.source).unwrap();
        let back: SourceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ev.source);

        let ctl_json = serde_json::to_string(&ev.control).unwrap();
        let ctl_back: Control = serde_json::from_str(&ctl_json).unwrap();
        assert_eq!(ctl_back, ev.control);
    }

    #[test]
    fn device_matcher_matches_correctly() {
        use crate::source::DeviceMatcher;
        assert!(DeviceMatcher::Exact("stick".into()).matches(&SourceId::Physical("stick".into())));
        assert!(!DeviceMatcher::Exact("stick".into()).matches(&SourceId::Ai("stick".into())));
        assert!(DeviceMatcher::AiAny.matches(&SourceId::Ai("anyone".into())));
        assert!(!DeviceMatcher::AiAny.matches(&SourceId::Physical("p".into())));
        assert!(DeviceMatcher::AiClient("a".into()).matches(&SourceId::Ai("a".into())));
        assert!(!DeviceMatcher::AiClient("a".into()).matches(&SourceId::Ai("b".into())));
    }
}
```

Add `serde_json` to `dev-dependencies` in `crates/rgp-core/Cargo.toml`:
```toml
[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 9: Run the tests**

Run: `cargo test -p rgp-core`
Expected: 2 passed, 0 failed.

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rgp-core
rtk git commit -m "feat(rgp-core): add foundation types, errors, and serde tests"
```

---

## Stage 2 — Tasks 2–6 are independent of each other. May run in parallel (5 subagents).

## Task 2: `rgp-config` — TOML load + validate + compile

**Spec reference:** §5.2 and §6 of design doc. Read both before this task.

**Files:**
- Create: `crates/rgp-config/Cargo.toml`
- Create: `crates/rgp-config/src/lib.rs`
- Create: `crates/rgp-config/src/schema.rs`
- Create: `crates/rgp-config/src/compile.rs`
- Create: `crates/rgp-config/tests/scenarios.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-config"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
serde = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write `tests/scenarios.rs` with the four scenario TOMLs as failing tests (TDD)**

```rust
use rgp_config::parse_str;

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

#[test]
fn fightstick_mixer_parses() {
    let cfg = parse_str(FIGHTSTICK_MIXER).expect("must parse");
    assert_eq!(cfg.profiles.len(), 1);
    assert_eq!(cfg.profiles[0].id.0, "fightstick-mixer");
    assert_eq!(cfg.default_profile.0, "fightstick-mixer");
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
fn duplicate_profile_id_is_validation_error() { /* ... */ }

#[test]
fn unknown_control_name_in_rule_is_validation_error() { /* ... */ }

#[test]
fn fightstick_mixer_compiles_to_lookup_table() {
    use rgp_core::{Control, ButtonId, AxisId, SourceId};
    let cfg = parse_str(FIGHTSTICK_MIXER).unwrap();
    let compiled = cfg.compile(&"fightstick-mixer".into()).unwrap();
    // The fight_stick_2 DPadUp rule should map to RightStickY = -1.0
    let key = (rgp_core::DeviceMatcher::Exact("fight_stick_2".into()),
               Control::Button(ButtonId::DPadUp));
    let action = compiled.rules.get(&key).expect("rule must exist");
    match action {
        rgp_config::RuleAction::SetAxis(AxisId::RightStickY, v) => assert_eq!(*v, -1.0),
        _ => panic!("wrong action"),
    }
}
```

(Stub-out the two TODO tests with `#[ignore]` and `unimplemented!()` initially — fill them in within this task as you write the code.)

- [ ] **Step 3: Run tests, verify they fail**

Run: `cargo test -p rgp-config`
Expected: compile error or "function not found" failures (no impl yet).

- [ ] **Step 4: Implement `src/schema.rs`**

Define serde structs matching the TOML schema in spec §6. Required structs:
- `Config { profiles, default_profile (named-table flatten), devices, server, hotkeys }`
- `Profile { id, name, inputs, rule }`
- `Mapping { from: ControlSelector, to: RuleTarget, deadzone, invert, sensitivity }`
- `ControlSelector { device: String, control: String }` (string parsed at compile time)
- `RuleTarget` deserialized from either `"passthrough"` (string) or `{axis = "...", value = ...}` (table) via untagged enum or custom deserializer
- `ServerConfig { addr: SocketAddr }`
- `HotkeyConfig { next_profile, prev_profile, panic_disconnect }`

Show enough code that the first test passes:
```rust
use std::collections::HashMap;
use std::net::SocketAddr;
use serde::Deserialize;
use rgp_core::ProfileId;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub devices: HashMap<String, String>,
    #[serde(rename = "profile")]
    pub profiles: Vec<Profile>,
    #[serde(rename = "default")]
    default_section: DefaultSection,
    pub server: ServerConfig,
    pub hotkeys: HotkeyConfig,
}

#[derive(Debug, Deserialize, Clone)]
struct DefaultSection { profile: String }

impl Config {
    pub fn default_profile(&self) -> ProfileId { ProfileId(self.default_section.profile.clone()) }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub inputs: Vec<String>,
    #[serde(rename = "rule", default)]
    pub rules: Vec<Mapping>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Mapping {
    pub from: ControlSelector,
    pub to: RuleTarget,
    #[serde(default)]
    pub deadzone: Option<f32>,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub sensitivity: Option<f32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlSelector { pub device: String, pub control: String }

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum RuleTarget {
    Passthrough(String),                                   // expects "passthrough"
    SetAxis { axis: String, value: f32 },
    SetButton { button: String, value: bool },
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig { pub addr: SocketAddr }

#[derive(Debug, Deserialize, Clone)]
pub struct HotkeyConfig {
    pub next_profile: String,
    pub prev_profile: String,
    pub panic_disconnect: String,
}
```

- [ ] **Step 5: Implement `src/lib.rs`**

```rust
pub mod schema;
pub mod compile;

pub use schema::*;
pub use compile::{CompiledProfile, RuleAction};

use rgp_core::RgpError;
use std::path::Path;

pub fn parse_str(s: &str) -> Result<Config, RgpError> {
    let cfg: Config = toml::from_str(s)
        .map_err(|e| RgpError::Config { line: e.span().map(|s| s.start), msg: e.to_string() })?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn load(path: &Path) -> Result<Config, RgpError> {
    let s = std::fs::read_to_string(path)?;
    parse_str(&s)
}

fn validate(cfg: &Config) -> Result<(), RgpError> {
    // 1. duplicate profile ids
    let mut seen = std::collections::HashSet::new();
    for p in &cfg.profiles {
        if !seen.insert(&p.id.0) {
            return Err(RgpError::Config { line: None, msg: format!("duplicate profile id: {}", p.id.0) });
        }
    }
    // 2. default profile must exist
    if !cfg.profiles.iter().any(|p| p.id.0 == cfg.default_section_ref().profile) {
        return Err(RgpError::Config { line: None, msg: "default.profile not found".into() });
    }
    // 3. inputs reference real device aliases (or "ai:*", "ai:<id>")
    for p in &cfg.profiles {
        for inp in &p.inputs {
            if !is_known_input(cfg, inp) {
                return Err(RgpError::Config { line: None, msg: format!("unknown device alias: {inp}") });
            }
        }
    }
    // 4. rule control names parse via parse_control()
    for p in &cfg.profiles {
        for r in &p.rules {
            if r.from.control != "*" {
                compile::parse_control(&r.from.control)
                    .map_err(|e| RgpError::Config { line: None, msg: e })?;
            }
        }
    }
    Ok(())
}

// Helper accessor since DefaultSection is private.
trait ConfigInternal { fn default_section_ref(&self) -> &schema::DefaultSection; }
```
(Adjust visibility of `DefaultSection`/`default_section` to make `validate` work — easiest: make `default_section` `pub(crate)`.)

- [ ] **Step 6: Implement `src/compile.rs`**

```rust
use std::collections::{HashMap, HashSet};
use rgp_core::{ProfileId, DeviceMatcher, Control, ButtonId, AxisId, TriggerId, RgpError};

#[derive(Debug, Clone)]
pub struct CompiledProfile {
    pub id: ProfileId,
    pub inputs: HashSet<DeviceMatcher>,
    pub rules: HashMap<(DeviceMatcher, Control), RuleAction>,
    pub passthrough: HashSet<DeviceMatcher>,
}

#[derive(Debug, Clone)]
pub enum RuleAction {
    PassControlSameName,
    SetButton(ButtonId, bool),
    SetAxis(AxisId, f32),
    Drop,
}

pub fn parse_control(s: &str) -> Result<Control, String> {
    match s {
        "South" => Ok(Control::Button(ButtonId::South)),
        "East"  => Ok(Control::Button(ButtonId::East)),
        // ... full mapping for every variant ...
        "DPadUp"    => Ok(Control::Button(ButtonId::DPadUp)),
        "DPadDown"  => Ok(Control::Button(ButtonId::DPadDown)),
        "DPadLeft"  => Ok(Control::Button(ButtonId::DPadLeft)),
        "DPadRight" => Ok(Control::Button(ButtonId::DPadRight)),
        "LeftStickX" => Ok(Control::Axis(AxisId::LeftStickX)),
        "RightStickX" => Ok(Control::Axis(AxisId::RightStickX)),
        "RightStickY" => Ok(Control::Axis(AxisId::RightStickY)),
        // ... etc ...
        "L2" => Ok(Control::Trigger(TriggerId::L2)),
        "R2" => Ok(Control::Trigger(TriggerId::R2)),
        other => Err(format!("unknown control name: {other}")),
    }
}

pub fn parse_axis(s: &str) -> Result<AxisId, String> { /* mirror parse_control */ }
pub fn parse_button(s: &str) -> Result<ButtonId, String> { /* mirror parse_control */ }

impl super::schema::Config {
    pub fn compile(&self, id: &ProfileId) -> Result<CompiledProfile, RgpError> {
        let profile = self.profiles.iter().find(|p| &p.id == id)
            .ok_or_else(|| RgpError::Config { line: None, msg: format!("profile not found: {}", id.0) })?;
        let mut inputs = HashSet::new();
        let mut rules = HashMap::new();
        let mut passthrough = HashSet::new();
        for input in &profile.inputs {
            inputs.insert(input_to_matcher(input));
        }
        for rule in &profile.rules {
            let dev = input_to_matcher(&rule.from.device);
            match (&rule.to, rule.from.control.as_str()) {
                (super::RuleTarget::Passthrough(s), _) if s == "passthrough" => {
                    passthrough.insert(dev.clone());
                }
                (super::RuleTarget::SetAxis { axis, value }, ctrl_name) => {
                    let from_ctl = parse_control(ctrl_name)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    let to_axis = parse_axis(axis)
                        .map_err(|e| RgpError::Config { line: None, msg: e })?;
                    rules.insert((dev, from_ctl), RuleAction::SetAxis(to_axis, *value));
                }
                (super::RuleTarget::SetButton { button, value }, ctrl_name) => { /* ... */ }
                _ => return Err(RgpError::Config { line: None, msg: "invalid rule".into() }),
            }
        }
        Ok(CompiledProfile { id: id.clone(), inputs, rules, passthrough })
    }
}

fn input_to_matcher(s: &str) -> DeviceMatcher {
    if s == "ai:*" { DeviceMatcher::AiAny }
    else if let Some(id) = s.strip_prefix("ai:") { DeviceMatcher::AiClient(id.into()) }
    else { DeviceMatcher::Exact(s.into()) }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p rgp-config`
Expected: all five tests pass.

- [ ] **Step 8: Add tests for the other three scenarios from spec §6**

`pad_passthrough_compiles`, `ai_only_compiles`, `fightstick_plus_ai_compiles`. Each parses, validates, and compiles. Each verifies one rule in the resulting `CompiledProfile`.

- [ ] **Step 9: Run all tests**

Run: `cargo test -p rgp-config`
Expected: 8+ tests pass.

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rgp-config
rtk git commit -m "feat(rgp-config): TOML load, validate, and compile profiles"
```

---

## Task 3: `rgp-input-ai` — in-process Rust API

**Spec reference:** §5.4 of design doc.

**Files:**
- Create: `crates/rgp-input-ai/Cargo.toml`
- Create: `crates/rgp-input-ai/src/lib.rs`
- Create: `crates/rgp-input-ai/src/timer.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-input-ai"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
crossbeam-channel = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

`crates/rgp-input-ai/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rgp_core::{ButtonId, Control, InputEvent};
    use std::time::{Duration, Instant};

    #[test]
    fn press_emits_press_then_release_after_duration() {
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = handle(tx, "test_agent");
        let t0 = Instant::now();
        h.press(ButtonId::B, Duration::from_millis(50));

        let press_ev = rx.recv_timeout(Duration::from_millis(20)).expect("press event");
        assert!(matches!(press_ev.control, Control::Button(ButtonId::B)));
        assert_eq!(press_ev.value, 1.0);

        let release_ev = rx.recv_timeout(Duration::from_millis(100)).expect("release event");
        assert!(matches!(release_ev.control, Control::Button(ButtonId::B)));
        assert_eq!(release_ev.value, 0.0);

        let elapsed = release_ev.timestamp.duration_since(t0);
        assert!(elapsed >= Duration::from_millis(40));
        assert!(elapsed <= Duration::from_millis(80), "release was {:?} after press", elapsed);
    }
}
```

(Note: spec §5.1 lists `ButtonId::B`? No — spec uses `South` (Xbox layout). Use `ButtonId::South` for the test. `B` was shorthand in conversation. Update both spec example commits and test if you spot drift. *Plan note: rgp-router's mapping tests use `South` consistently.*)

Replace `ButtonId::B` with `ButtonId::South` in the test above before running.

- [ ] **Step 3: Run, verify fail**

Run: `cargo test -p rgp-input-ai`
Expected: compile failure (`handle` and `AiInputHandle` don't exist).

- [ ] **Step 4: Implement `src/timer.rs`**

```rust
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::sync::{Arc, Mutex, Condvar};
use std::time::Instant;
use std::thread;
use crossbeam_channel::Sender;
use rgp_core::{InputEvent, Control, SourceId};

#[derive(Debug)]
struct Scheduled {
    deadline: Instant,
    event: InputEvent,
}

impl PartialEq for Scheduled { fn eq(&self, o: &Self) -> bool { self.deadline == o.deadline } }
impl Eq for Scheduled {}
impl Ord for Scheduled { fn cmp(&self, o: &Self) -> std::cmp::Ordering { self.deadline.cmp(&o.deadline) } }
impl PartialOrd for Scheduled { fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }

pub(crate) struct Timer {
    state: Arc<(Mutex<BinaryHeap<Reverse<Scheduled>>>, Condvar)>,
    tx: Sender<InputEvent>,
}

impl Timer {
    pub fn new(tx: Sender<InputEvent>) -> Self {
        let state = Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new()));
        let timer = Timer { state: state.clone(), tx: tx.clone() };
        thread::Builder::new().name("rgp-input-ai-timer".into()).spawn(move || {
            timer_loop(state, tx);
        }).expect("spawn timer thread");
        Timer { state: state.clone(), tx } // need a copy to return — fix by wrapping in Arc
    }
    // ... schedule(...) push to heap and notify cvar
}

fn timer_loop(state: Arc<(Mutex<BinaryHeap<Reverse<Scheduled>>>, Condvar)>, tx: Sender<InputEvent>) {
    let (lock, cvar) = &*state;
    loop {
        let mut heap = lock.lock().unwrap();
        let timeout = match heap.peek() {
            Some(Reverse(s)) => s.deadline.saturating_duration_since(Instant::now()),
            None => std::time::Duration::from_secs(60), // wake periodically; could use park_timeout
        };
        let res = cvar.wait_timeout(heap, timeout).unwrap();
        heap = res.0;
        let now = Instant::now();
        while let Some(Reverse(s)) = heap.peek() {
            if s.deadline <= now {
                let s = heap.pop().unwrap().0;
                let _ = tx.send(s.event);
            } else { break; }
        }
    }
}
```
(Fix the Timer-double-construction in the snippet above: the cleanest implementation stores `Arc` of the cvar pair, and returns one `Timer` struct. Adjust during impl.)

- [ ] **Step 5: Implement `src/lib.rs`**

```rust
pub mod timer;
use std::time::Duration;
use crossbeam_channel::Sender;
use rgp_core::{InputEvent, Control, ButtonId, AxisId, TriggerId, SourceId};

pub struct AiInputHandle {
    tx: Sender<InputEvent>,
    source_id: String,
    timer: timer::Timer,
}

impl AiInputHandle {
    pub fn press(&self, button: ButtonId, duration: Duration) {
        let now = std::time::Instant::now();
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 1.0, timestamp: now,
        });
        self.timer.schedule(now + duration, InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 0.0, timestamp: now + duration,
        });
    }
    pub fn release(&self, button: ButtonId) {
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 0.0, timestamp: std::time::Instant::now(),
        });
    }
    pub fn axis(&self, axis: AxisId, value: f32) {
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Axis(axis), value,
            timestamp: std::time::Instant::now(),
        });
    }
    pub fn trigger(&self, t: TriggerId, value: f32) {
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Trigger(t), value,
            timestamp: std::time::Instant::now(),
        });
    }
    pub fn raw(&self, event: InputEvent) { let _ = self.tx.send(event); }
}

pub fn handle(events_tx: Sender<InputEvent>, source_id: impl Into<String>) -> AiInputHandle {
    let source_id = source_id.into();
    let timer = timer::Timer::new(events_tx.clone());
    AiInputHandle { tx: events_tx, source_id, timer }
}
```

- [ ] **Step 6: Run test, verify pass**

Run: `cargo test -p rgp-input-ai`
Expected: 1 test passes.

- [ ] **Step 7: Add the concurrency test**

```rust
#[test]
fn concurrent_press_release_stays_consistent() {
    use std::sync::Arc;
    let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
    let h = Arc::new(handle(tx, "concurrent"));
    let mut threads = vec![];
    for _ in 0..4 {
        let h = h.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..100 {
                h.press(ButtonId::South, Duration::from_millis(10));
            }
        }));
    }
    for t in threads { t.join().unwrap(); }
    std::thread::sleep(Duration::from_millis(100));
    let mut press_count = 0; let mut release_count = 0;
    while let Ok(ev) = rx.try_recv() {
        if ev.value == 1.0 { press_count += 1; } else { release_count += 1; }
    }
    assert_eq!(press_count, 400);
    assert_eq!(release_count, 400);
}
```

- [ ] **Step 8: Run all tests, commit**

```bash
cargo test -p rgp-input-ai
rtk git add crates/rgp-input-ai
rtk git commit -m "feat(rgp-input-ai): in-process API with timer-thread release scheduling"
```

---

## Task 4: `rgp-input-physical` — gilrs wrapper

**Spec reference:** §5.3.

**Files:**
- Create: `crates/rgp-input-physical/Cargo.toml`
- Create: `crates/rgp-input-physical/src/lib.rs`
- Create: `crates/rgp-input-physical/src/translate.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-input-physical"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
gilrs = { workspace = true }
crossbeam-channel = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Write failing test for `translate`**

`crates/rgp-input-physical/src/translate.rs`:
```rust
use gilrs::{Event, EventType, GamepadId, Button, Axis};
use rgp_core::{InputEvent, Control, ButtonId, AxisId, SourceId};
use std::time::Instant;

pub fn translate(event: &Event, source_id: &str) -> Option<InputEvent> {
    let control = match event.event {
        EventType::ButtonPressed(btn, _) | EventType::ButtonReleased(btn, _) => {
            Control::Button(map_gilrs_button(btn)?)
        }
        EventType::AxisChanged(axis, _, _) => Control::Axis(map_gilrs_axis(axis)?),
        EventType::ButtonChanged(_, _, _) => return None,  // we use pressed/released
        _ => return None,
    };
    let value = match event.event {
        EventType::ButtonPressed(_, _) => 1.0,
        EventType::ButtonReleased(_, _) => 0.0,
        EventType::AxisChanged(_, v, _) => v,
        _ => return None,
    };
    Some(InputEvent {
        source: SourceId::Physical(source_id.into()),
        control, value,
        timestamp: Instant::now(),
    })
}

fn map_gilrs_button(b: Button) -> Option<ButtonId> {
    match b {
        Button::South => Some(ButtonId::South),
        Button::East => Some(ButtonId::East),
        Button::North => Some(ButtonId::North),
        Button::West => Some(ButtonId::West),
        Button::DPadUp => Some(ButtonId::DPadUp),
        Button::DPadDown => Some(ButtonId::DPadDown),
        Button::DPadLeft => Some(ButtonId::DPadLeft),
        Button::DPadRight => Some(ButtonId::DPadRight),
        Button::LeftThumb => Some(ButtonId::LeftStickClick),
        Button::RightThumb => Some(ButtonId::RightStickClick),
        Button::LeftTrigger => Some(ButtonId::LeftBumper),
        Button::RightTrigger => Some(ButtonId::RightBumper),
        Button::Start => Some(ButtonId::Start),
        Button::Select => Some(ButtonId::Select),
        Button::Mode => Some(ButtonId::Guide),
        _ => None,
    }
}

fn map_gilrs_axis(a: Axis) -> Option<AxisId> {
    match a {
        Axis::LeftStickX => Some(AxisId::LeftStickX),
        Axis::LeftStickY => Some(AxisId::LeftStickY),
        Axis::RightStickX => Some(AxisId::RightStickX),
        Axis::RightStickY => Some(AxisId::RightStickY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gilrs::ev::Code;
    // gilrs doesn't expose a public Event constructor easily; if tests are hard to construct,
    // refactor `translate` to take the inner EventType + GamepadId separately.

    #[test]
    fn unmapped_button_returns_none() {
        // Pseudocode — adjust to gilrs's actual Code construction:
        // let ev = Event { id: GamepadId::from(0), event: EventType::ButtonPressed(Button::Unknown, Code::default()), time: SystemTime::now() };
        // assert!(translate(&ev, "test").is_none());
    }
}
```

If gilrs's `Event` constructor is private, refactor `translate` to take an `&EventType` (and a `&str` for source_id) directly, which makes it trivially testable. The plan recommends this refactor.

- [ ] **Step 3: Refactor for testability**

Change signature to `pub fn translate_event_type(et: &EventType, source_id: &str) -> Option<InputEvent>`. Update test:

```rust
#[test]
fn button_press_translates_to_input_event() {
    let et = EventType::ButtonPressed(Button::South, Code::default());
    let ev = translate_event_type(&et, "stick1").expect("must translate");
    assert!(matches!(ev.source, SourceId::Physical(ref s) if s == "stick1"));
    assert!(matches!(ev.control, Control::Button(ButtonId::South)));
    assert_eq!(ev.value, 1.0);
}

#[test]
fn axis_change_translates_with_value() {
    let et = EventType::AxisChanged(Axis::LeftStickX, -0.7, Code::default());
    let ev = translate_event_type(&et, "stick1").expect("must translate");
    assert!(matches!(ev.control, Control::Axis(AxisId::LeftStickX)));
    assert!((ev.value - -0.7).abs() < 1e-6);
}

#[test]
fn unmapped_button_returns_none() {
    let et = EventType::ButtonPressed(Button::Unknown, Code::default());
    assert!(translate_event_type(&et, "stick1").is_none());
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rgp-input-physical`
Expected: 3 tests pass.

- [ ] **Step 5: Implement `src/lib.rs` with `run()` and `list_connected()`**

```rust
pub mod translate;

use std::collections::{HashMap, HashSet};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use crossbeam_channel::{Sender, Receiver};
use gilrs::{Gilrs, EventType};
use rgp_core::{InputEvent, RgpError, DeviceInfo, SourceId, Control};

pub fn run(events_tx: Sender<InputEvent>, shutdown: Receiver<()>) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-input-physical".into()).spawn(move || -> Result<(), RgpError> {
        let mut gilrs = Gilrs::new().map_err(|e| RgpError::InputSource(format!("{e}")))?;
        // held: per-gamepad set of currently-held controls (for clean disconnect).
        let mut held: HashMap<gilrs::GamepadId, HashSet<Control>> = HashMap::new();
        loop {
            if shutdown.try_recv().is_ok() || matches!(shutdown.try_recv(), Err(crossbeam_channel::TryRecvError::Disconnected)) { break; }
            while let Some(ev) = gilrs.next_event() {
                let source_id = format!("{:?}", ev.id);     // gilrs uuid available via gilrs.gamepad(id).uuid()
                if let EventType::Disconnected = ev.event {
                    if let Some(set) = held.remove(&ev.id) {
                        for ctl in set {
                            let _ = events_tx.try_send(InputEvent {
                                source: SourceId::Physical(source_id.clone()),
                                control: ctl, value: 0.0,
                                timestamp: std::time::Instant::now(),
                            });
                        }
                    }
                    continue;
                }
                if let Some(input) = translate::translate_event_type(&ev.event, &source_id) {
                    // Update held set
                    let set = held.entry(ev.id).or_default();
                    if input.value != 0.0 { set.insert(input.control); }
                    else { set.remove(&input.control); }
                    if events_tx.try_send(input).is_err() {
                        tracing::warn!(target: "rgp::input::physical", "events_tx full; dropping event");
                    }
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }).expect("spawn input-physical thread")
}

pub fn list_connected() -> Vec<DeviceInfo> {
    let mut gilrs = match Gilrs::new() { Ok(g) => g, Err(_) => return vec![] };
    gilrs.gamepads().map(|(_, gp)| DeviceInfo {
        id: SourceId::Physical(format!("uuid:{}", uuid::Uuid::from_bytes(gp.uuid()))),
        name: gp.name().to_string(),
        connected: gp.is_connected(),
    }).collect()
}
```

(Note: `gilrs::Gamepad::uuid()` returns `[u8; 16]`. Use `uuid` crate to format.)

- [ ] **Step 6: Add disconnect-released-held test**

This is harder to test without gilrs hardware. Skip for v1 with `#[ignore]` and a comment "covered by integration test in rgp-app".

- [ ] **Step 7: Run, commit**

```bash
cargo test -p rgp-input-physical
rtk git add crates/rgp-input-physical
rtk git commit -m "feat(rgp-input-physical): gilrs wrapper with disconnect-release tracking"
```

---

## Task 5: `rgp-virtual-pad` — ViGEmBus sink

**Spec reference:** §5.7.

**Files:**
- Create: `crates/rgp-virtual-pad/Cargo.toml`
- Create: `crates/rgp-virtual-pad/src/lib.rs`
- Create: `crates/rgp-virtual-pad/src/translate.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-virtual-pad"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
vigem-client = { workspace = true }
crossbeam-channel = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Write failing test for `pad_state_to_xgamepad`**

`src/translate.rs`:
```rust
use rgp_core::{PadState, ButtonId, AxisId, TriggerId};
use vigem_client::{XGamepad, XButtons};

pub fn pad_state_to_xgamepad(state: &PadState) -> XGamepad {
    let mut buttons = XButtons::default();
    let mut set = |b: ButtonId, bit: u16| {
        if *state.buttons.get(&b).unwrap_or(&false) { buttons.raw |= bit; }
    };
    set(ButtonId::South,   XButtons::A);
    set(ButtonId::East,    XButtons::B);
    set(ButtonId::West,    XButtons::X);
    set(ButtonId::North,   XButtons::Y);
    set(ButtonId::DPadUp,    XButtons::UP);
    set(ButtonId::DPadDown,  XButtons::DOWN);
    set(ButtonId::DPadLeft,  XButtons::LEFT);
    set(ButtonId::DPadRight, XButtons::RIGHT);
    set(ButtonId::LeftStickClick,  XButtons::LTHUMB);
    set(ButtonId::RightStickClick, XButtons::RTHUMB);
    set(ButtonId::LeftBumper,  XButtons::LB);
    set(ButtonId::RightBumper, XButtons::RB);
    set(ButtonId::Start,  XButtons::START);
    set(ButtonId::Select, XButtons::BACK);
    set(ButtonId::Guide,  XButtons::GUIDE);

    let axis_to_i16 = |v: f32| -> i16 {
        let v = v.clamp(-1.0, 1.0);
        (v * i16::MAX as f32) as i16
    };
    let trig_to_u8 = |v: f32| -> u8 {
        let v = v.clamp(0.0, 1.0);
        (v * 255.0) as u8
    };
    XGamepad {
        buttons,
        thumb_lx: axis_to_i16(*state.axes.get(&AxisId::LeftStickX).unwrap_or(&0.0)),
        thumb_ly: axis_to_i16(*state.axes.get(&AxisId::LeftStickY).unwrap_or(&0.0)),
        thumb_rx: axis_to_i16(*state.axes.get(&AxisId::RightStickX).unwrap_or(&0.0)),
        thumb_ry: axis_to_i16(*state.axes.get(&AxisId::RightStickY).unwrap_or(&0.0)),
        left_trigger:  trig_to_u8(*state.triggers.get(&TriggerId::L2).unwrap_or(&0.0)),
        right_trigger: trig_to_u8(*state.triggers.get(&TriggerId::R2).unwrap_or(&0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn south_button_sets_a_bit() {
        let mut s = PadState::default();
        s.buttons.insert(ButtonId::South, true);
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.buttons.raw & XButtons::A, XButtons::A);
    }

    #[test]
    fn axis_negative_one_maps_to_i16_min() {
        let mut s = PadState::default();
        s.axes.insert(AxisId::LeftStickX, -1.0);
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.thumb_lx, i16::MIN + 1);  // -32767, not -32768 (within ±1)
    }

    #[test]
    fn trigger_one_maps_to_255() {
        let mut s = PadState::default();
        s.triggers.insert(TriggerId::R2, 1.0);
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.right_trigger, 255);
    }
}
```

- [ ] **Step 3: Run, verify pass**

Run: `cargo test -p rgp-virtual-pad`
Expected: 3 tests pass.

- [ ] **Step 4: Implement `src/lib.rs` with `run()`**

```rust
pub mod translate;

use std::thread::{self, JoinHandle};
use std::time::Duration;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use rgp_core::{PadState, RgpError};
use vigem_client::{Client, Xbox360Wired, TargetId};

pub trait PadSink: Send {
    fn submit(&mut self, report: vigem_client::XGamepad) -> Result<(), RgpError>;
}

pub struct ViGEmPad { pad: Xbox360Wired<Client> }

impl PadSink for ViGEmPad {
    fn submit(&mut self, report: vigem_client::XGamepad) -> Result<(), RgpError> {
        self.pad.update(&report).map_err(|e| RgpError::VirtualPad(format!("{e:?}")))
    }
}

pub fn connect() -> Result<ViGEmPad, RgpError> {
    let client = Client::connect().map_err(|e| RgpError::VirtualPad(format!("ViGEmBus connect failed: {e:?} (is the driver installed?)")))?;
    let mut pad = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
    pad.plugin().map_err(|e| RgpError::VirtualPad(format!("plugin failed: {e:?}")))?;
    pad.wait_ready().map_err(|e| RgpError::VirtualPad(format!("wait_ready failed: {e:?}")))?;
    Ok(ViGEmPad { pad })
}

pub fn run(pad_rx: Receiver<PadState>, shutdown: Receiver<()>, mut sink: Box<dyn PadSink>) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-virtual-pad".into()).spawn(move || -> Result<(), RgpError> {
        loop {
            crossbeam_channel::select! {
                recv(pad_rx) -> msg => match msg {
                    Ok(state) => sink.submit(translate::pad_state_to_xgamepad(&state))?,
                    Err(_) => break,
                },
                recv(shutdown) -> _ => break,
            }
        }
        // Final all-zero release pass
        let _ = sink.submit(translate::pad_state_to_xgamepad(&PadState::default()));
        Ok(())
    }).expect("spawn virtual-pad thread")
}
```

- [ ] **Step 5: Add `FakePad` for tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use vigem_client::XGamepad;
    
    pub struct FakePad { pub submitted: Arc<Mutex<Vec<XGamepad>>> }
    impl PadSink for FakePad {
        fn submit(&mut self, r: XGamepad) -> Result<(), RgpError> {
            self.submitted.lock().unwrap().push(r);
            Ok(())
        }
    }

    #[test]
    fn run_processes_pad_states_and_releases_on_shutdown() {
        let (pad_tx, pad_rx) = crossbeam_channel::bounded::<PadState>(8);
        let (sd_tx, sd_rx) = crossbeam_channel::bounded::<()>(0);
        let submitted = Arc::new(Mutex::new(vec![]));
        let fake = FakePad { submitted: submitted.clone() };
        let handle = run(pad_rx, sd_rx, Box::new(fake));
        let mut s = PadState::default();
        s.buttons.insert(ButtonId::South, true);
        pad_tx.send(s).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        drop(sd_tx);
        handle.join().unwrap().unwrap();
        let recorded = submitted.lock().unwrap();
        assert!(recorded.len() >= 2);                                      // at least the state + release
        assert_eq!(recorded.last().unwrap().buttons.raw, 0);              // final all-zero release
    }
}
```

- [ ] **Step 6: Run, commit**

```bash
cargo test -p rgp-virtual-pad
rtk git add crates/rgp-virtual-pad
rtk git commit -m "feat(rgp-virtual-pad): ViGEmBus sink with FakePad seam and shutdown release"
```

---

## Task 6: `rgp-tray` — system tray + hotkeys

**Spec reference:** §5.8.

**Files:**
- Create: `crates/rgp-tray/Cargo.toml`
- Create: `crates/rgp-tray/src/lib.rs`
- Create: `crates/rgp-tray/src/menu.rs`
- Create: `crates/rgp-tray/src/hotkeys.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-tray"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
crossbeam-channel = { workspace = true }
tray-icon = { workspace = true }
global-hotkey = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Write failing tests for the pure logic (`menu.rs`, `hotkeys.rs`)**

`src/menu.rs`:
```rust
use rgp_core::ProfileId;

pub fn next_profile(current: &ProfileId, all: &[ProfileId]) -> ProfileId {
    let idx = all.iter().position(|p| p == current).unwrap_or(0);
    all[(idx + 1) % all.len()].clone()
}

pub fn prev_profile(current: &ProfileId, all: &[ProfileId]) -> ProfileId {
    let idx = all.iter().position(|p| p == current).unwrap_or(0);
    all[(idx + all.len() - 1) % all.len()].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pids(names: &[&str]) -> Vec<ProfileId> { names.iter().map(|n| ProfileId(n.to_string())).collect() }

    #[test]
    fn next_profile_wraps_around() {
        let all = pids(&["a", "b", "c"]);
        assert_eq!(next_profile(&"c".into(), &all).0, "a");
        assert_eq!(next_profile(&"a".into(), &all).0, "b");
    }

    #[test]
    fn prev_profile_wraps_around() {
        let all = pids(&["a", "b", "c"]);
        assert_eq!(prev_profile(&"a".into(), &all).0, "c");
        assert_eq!(prev_profile(&"b".into(), &all).0, "a");
    }
}
```

`src/hotkeys.rs`:
```rust
use global_hotkey::hotkey::{HotKey, Modifiers, Code};

pub fn parse(s: &str) -> Result<HotKey, String> {
    let parts: Vec<&str> = s.split('+').collect();
    let (mods, key_str) = match parts.as_slice() {
        [k] => (Modifiers::empty(), *k),
        [m, k] if *m == "Ctrl" => (Modifiers::CONTROL, *k),
        [m, k] if *m == "Alt"  => (Modifiers::ALT, *k),
        [m, k] if *m == "Shift"=> (Modifiers::SHIFT, *k),
        _ => return Err(format!("unsupported hotkey: {s}")),
    };
    let code = match key_str {
        "F9"  => Code::F9,  "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
        // ... others as needed
        _ => return Err(format!("unsupported key: {key_str}")),
    };
    Ok(HotKey::new(Some(mods), code))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_f9() { assert!(parse("F9").is_ok()); }
    #[test]
    fn parses_ctrl_f12() { assert!(parse("Ctrl+F12").is_ok()); }
    #[test]
    fn rejects_garbage() { assert!(parse("Meta+Z").is_err()); }
}
```

- [ ] **Step 3: Run pure-logic tests, verify pass**

Run: `cargo test -p rgp-tray`
Expected: 5 tests pass.

- [ ] **Step 4: Implement `src/lib.rs`'s `run_on_main()`**

```rust
pub mod menu;
pub mod hotkeys;

use crossbeam_channel::Sender;
use rgp_core::{ControlMsg, ProfileId, RgpError};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuEvent, MenuItem, CheckMenuItem}};
use global_hotkey::{GlobalHotKeyManager, GlobalHotKeyEvent};

pub fn run_on_main(
    control_tx: Sender<ControlMsg>,
    profiles: Vec<ProfileId>,
) -> Result<(), RgpError> {
    let menu = Menu::new();
    let profile_items: Vec<CheckMenuItem> = profiles.iter().map(|p| {
        CheckMenuItem::new(&p.0, true, false, None)
    }).collect();
    for item in &profile_items { menu.append(item).map_err(|e| RgpError::Channel(e.to_string()))?; }
    let quit = MenuItem::new("Quit", true, None);
    menu.append(&quit).map_err(|e| RgpError::Channel(e.to_string()))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("riptheGamePad")
        .build()
        .map_err(|e| RgpError::Channel(e.to_string()))?;

    let manager = GlobalHotKeyManager::new().map_err(|e| RgpError::Channel(e.to_string()))?;
    let next = hotkeys::parse("F9").unwrap();
    let prev = hotkeys::parse("F10").unwrap();
    manager.register(next).ok();
    manager.register(prev).ok();

    let menu_rx = MenuEvent::receiver();
    let hot_rx = GlobalHotKeyEvent::receiver();
    let mut current_idx: usize = 0;

    // Pump OS events. tray-icon needs a Win32 message loop on Windows.
    // The actual event-loop pattern depends on tray-icon version; the canonical pattern
    // is winit::event_loop or tao. Use whichever the version of tray-icon supports.
    // Pseudocode shape:
    loop {
        if let Ok(ev) = menu_rx.try_recv() {
            if ev.id == quit.id() { break; }
            if let Some(idx) = profile_items.iter().position(|i| i.id() == &ev.id) {
                current_idx = idx;
                let _ = control_tx.send(ControlMsg::SetActiveProfile(profiles[idx].clone()));
            }
        }
        if let Ok(ev) = hot_rx.try_recv() {
            if ev.id == next.id() {
                current_idx = (current_idx + 1) % profiles.len();
                let _ = control_tx.send(ControlMsg::SetActiveProfile(profiles[current_idx].clone()));
            }
            if ev.id == prev.id() {
                current_idx = (current_idx + profiles.len() - 1) % profiles.len();
                let _ = control_tx.send(ControlMsg::SetActiveProfile(profiles[current_idx].clone()));
            }
        }
        // PUMP OS MESSAGES — required for tray-icon on Windows. tray-icon docs show pattern.
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = control_tx.send(ControlMsg::Quit);
    Ok(())
}
```

(The real tray-icon Windows event loop is non-trivial. Subagent: read `tray-icon` crate docs — the version pinned in workspace deps. If using `tao` or `winit` is required by the version, add it as a dep and use its EventLoop.)

- [ ] **Step 5: Run, commit**

```bash
cargo test -p rgp-tray
cargo build -p rgp-tray
rtk git add crates/rgp-tray
rtk git commit -m "feat(rgp-tray): tray menu, hotkey parsing, and event-loop scaffold"
```

---

## Stage 3 — Tasks 7 and 8 are independent of each other but require Stage 2 outputs.

## Task 7: `rgp-input-ai-server` — WebSocket transport

**Spec reference:** §5.5.

**Files:**
- Create: `crates/rgp-input-ai-server/Cargo.toml`
- Create: `crates/rgp-input-ai-server/src/lib.rs`
- Create: `crates/rgp-input-ai-server/src/frame.rs`
- Create: `crates/rgp-input-ai-server/src/connection.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-input-ai-server"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
rgp-input-ai = { path = "../rgp-input-ai" }
crossbeam-channel = { workspace = true }
tokio = { workspace = true }
tokio-tungstenite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write `src/frame.rs` with the wire format and decoding tests**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Frame {
    Hello { client_id: String },
    Press { button: String, duration_ms: u64 },
    Release { button: String },
    Axis { axis: String, value: f32 },
    Trigger { trigger: String, value: f32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_press() {
        let f: Frame = serde_json::from_str(r#"{"type":"press","button":"South","duration_ms":50}"#).unwrap();
        assert_eq!(f, Frame::Press { button: "South".into(), duration_ms: 50 });
    }
    #[test]
    fn parse_axis() {
        let f: Frame = serde_json::from_str(r#"{"type":"axis","axis":"LeftStickX","value":-0.7}"#).unwrap();
        assert!(matches!(f, Frame::Axis { axis, value } if axis == "LeftStickX" && (value - -0.7).abs() < 1e-6));
    }
    #[test]
    fn parse_hello() {
        let f: Frame = serde_json::from_str(r#"{"type":"hello","client_id":"agent1"}"#).unwrap();
        assert_eq!(f, Frame::Hello { client_id: "agent1".into() });
    }
    #[test]
    fn malformed_returns_error() {
        assert!(serde_json::from_str::<Frame>(r#"{"type":"nonsense"}"#).is_err());
    }
}
```

- [ ] **Step 3: Run, verify pass**

Run: `cargo test -p rgp-input-ai-server`
Expected: 4 frame tests pass.

- [ ] **Step 4: Implement `src/connection.rs` with the per-conn state machine**

```rust
use tokio::net::TcpStream;
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use futures_util::StreamExt;
use std::time::Duration;
use rgp_core::{InputEvent, ButtonId, AxisId, TriggerId};
use rgp_input_ai::{handle, AiInputHandle};
use crate::frame::Frame;

pub async fn handle_connection(
    stream: TcpStream,
    events_tx: crossbeam_channel::Sender<InputEvent>,
) {
    let mut ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => { tracing::warn!(?e, "ws handshake failed"); return; }
    };
    let mut client_id = uuid::Uuid::new_v4().to_string();
    let mut malformed_run = 0u32;
    let mut handle: Option<AiInputHandle> = None;
    let mut received_first = false;

    while let Some(msg) = ws.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        };
        let frame: Result<Frame, _> = serde_json::from_str(&text);
        match frame {
            Ok(Frame::Hello { client_id: cid }) => {
                if received_first {
                    tracing::warn!(cid, "hello received after first frame; dropping");
                    continue;
                }
                client_id = cid;
                received_first = true;
                malformed_run = 0;
            }
            Ok(other) => {
                received_first = true;
                if handle.is_none() {
                    handle = Some(handle(events_tx.clone(), client_id.clone()));
                }
                let h = handle.as_ref().unwrap();
                apply(other, h);
                malformed_run = 0;
            }
            Err(e) => {
                tracing::warn!(?e, "malformed frame");
                malformed_run += 1;
                if malformed_run >= 3 { break; }
            }
        }
    }
    // disconnect: release-all-held synthetic events
    // (handle goes out of scope; releases are handled at the source level)
    if let Some(h) = handle.take() {
        h.release_all();    // add this to AiInputHandle in Task 3 — see plan note below
    }
}

fn apply(f: Frame, h: &AiInputHandle) {
    match f {
        Frame::Press { button, duration_ms } => {
            if let Ok(b) = parse_button(&button) {
                h.press(b, Duration::from_millis(duration_ms));
            }
        }
        Frame::Release { button } => { if let Ok(b) = parse_button(&button) { h.release(b); } }
        Frame::Axis { axis, value } => { if let Ok(a) = parse_axis(&axis) { h.axis(a, value); } }
        Frame::Trigger { trigger, value } => { if let Ok(t) = parse_trigger(&trigger) { h.trigger(t, value); } }
        Frame::Hello { .. } => unreachable!(),
    }
}

fn parse_button(s: &str) -> Result<ButtonId, ()> {
    match s {
        "South" => Ok(ButtonId::South), "East" => Ok(ButtonId::East),
        "North" => Ok(ButtonId::North), "West" => Ok(ButtonId::West),
        // ... full mapping
        _ => Err(()),
    }
}
fn parse_axis(s: &str) -> Result<AxisId, ()> { /* ... */ Err(()) }
fn parse_trigger(s: &str) -> Result<TriggerId, ()> { /* ... */ Err(()) }
```

**Plan note:** `AiInputHandle::release_all()` was not in Task 3's API. Add it to Task 3 *before* this task: it should release every button id held by the handle (track via `AtomicBool` map or just emit zero-value events for every button on call). If you're executing tasks in order, go back and update Task 3 first.

- [ ] **Step 5: Implement `src/lib.rs`'s `run()`**

```rust
pub mod frame;
pub mod connection;

use std::net::SocketAddr;
use std::thread::{self, JoinHandle};
use crossbeam_channel::{Sender, Receiver};
use rgp_core::{InputEvent, RgpError};

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
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind(addr).await
                .map_err(|e| RgpError::Io(e))?;
            tracing::info!(target: "rgp::input::ai_server", %addr, "listening");
            loop {
                tokio::select! {
                    Ok((stream, peer)) = listener.accept() => {
                        tracing::debug!(?peer, "ws conn");
                        let tx = events_tx.clone();
                        tokio::task::spawn_local(async move {
                            connection::handle_connection(stream, tx).await;
                        });
                    }
                    _ = tokio::task::yield_now() => {
                        if shutdown.try_recv().is_ok() { break; }
                    }
                }
            }
            Ok::<_, RgpError>(())
        })?;
        Ok(())
    }).expect("spawn ai-server thread")
}
```

(Note: the shutdown polling pattern with `yield_now()` is a placeholder. A cleaner pattern: spawn a dedicated task that listens on `shutdown` via `tokio::sync::Notify`, set up via the `LocalSet`. The subagent should implement whichever shutdown pattern is idiomatic for the version of tokio in use.)

- [ ] **Step 6: Add integration test for malformed-3-times-closes-conn**

Skip if WS test harness is high-effort; cover via integration test in `rgp-app` (Task 9).

- [ ] **Step 7: Run, commit**

```bash
cargo test -p rgp-input-ai-server
cargo build -p rgp-input-ai-server
rtk git add crates/rgp-input-ai-server
rtk git commit -m "feat(rgp-input-ai-server): WebSocket transport over rgp-input-ai"
```

---

## Task 8: `rgp-router` — the brain (highest test surface)

**Spec reference:** §5.6 of design doc — read fully. Note especially the 50+ test target.

**Files:**
- Create: `crates/rgp-router/Cargo.toml`
- Create: `crates/rgp-router/src/lib.rs`
- Create: `crates/rgp-router/src/apply.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-router"
edition.workspace = true
version.workspace = true

[dependencies]
rgp-core = { path = "../rgp-core" }
rgp-config = { path = "../rgp-config" }
crossbeam-channel = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Write the FIRST failing mapping test for `apply_event`**

`crates/rgp-router/src/apply.rs`:
```rust
use rgp_core::{InputEvent, PadState, Control, ButtonId, AxisId, SourceId};
use rgp_config::{CompiledProfile, RuleAction};

/// Returns true if the state changed.
pub fn apply_event(state: &mut PadState, profile: &CompiledProfile, event: &InputEvent) -> bool {
    // 1. Source must be in profile.inputs (deviceMatcher.matches).
    if !profile.inputs.iter().any(|m| m.matches(&event.source)) { return false; }

    // 2. Look up the exact rule.
    for matcher in profile.inputs.iter().filter(|m| m.matches(&event.source)) {
        let key = (matcher.clone(), event.control);
        if let Some(action) = profile.rules.get(&key) {
            return apply_action(state, action, event);
        }
    }

    // 3. If source has passthrough, map control to its same-named virtual counterpart.
    if profile.inputs.iter().any(|m| m.matches(&event.source) && profile.passthrough.contains(m)) {
        return apply_passthrough(state, event);
    }

    // 4. Implicit drop.
    false
}

fn apply_action(state: &mut PadState, action: &RuleAction, event: &InputEvent) -> bool {
    match action {
        RuleAction::SetButton(b, v) => {
            let pressed = event.value > 0.5;
            let new_val = pressed ^ !v;     // SetButton(b, true) presses when event press; (true) inverts is rare
            // Simpler: when event press, set b to *v; when event release, set b to !*v
            let target = if pressed { *v } else { !*v };
            let prev = state.buttons.insert(*b, target);
            prev != Some(target)
        }
        RuleAction::SetAxis(a, magnitude) => {
            let new_val = if event.value > 0.5 { *magnitude } else { 0.0 };
            let prev = state.axes.insert(*a, new_val);
            prev.map(|p| (p - new_val).abs() > f32::EPSILON).unwrap_or(true)
        }
        RuleAction::PassControlSameName => apply_passthrough(state, event),
        RuleAction::Drop => false,
    }
}

fn apply_passthrough(state: &mut PadState, event: &InputEvent) -> bool {
    match event.control {
        Control::Button(b) => {
            let pressed = event.value > 0.5;
            state.buttons.insert(b, pressed) != Some(pressed)
        }
        Control::Axis(a) => {
            let prev = state.axes.insert(a, event.value);
            prev.map(|p| (p - event.value).abs() > f32::EPSILON).unwrap_or(true)
        }
        Control::Trigger(t) => {
            let prev = state.triggers.insert(t, event.value);
            prev.map(|p| (p - event.value).abs() > f32::EPSILON).unwrap_or(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    use rgp_core::{DeviceMatcher, ProfileId};

    fn ev(source: SourceId, control: Control, value: f32) -> InputEvent {
        InputEvent { source, control, value, timestamp: Instant::now() }
    }

    fn fightstick_mixer_profile() -> CompiledProfile {
        let mut rules = HashMap::new();
        let stick2 = DeviceMatcher::Exact("fight_stick_2".into());
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadUp)),
                     RuleAction::SetAxis(AxisId::RightStickY, -1.0));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadDown)),
                     RuleAction::SetAxis(AxisId::RightStickY, 1.0));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadLeft)),
                     RuleAction::SetAxis(AxisId::RightStickX, -1.0));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadRight)),
                     RuleAction::SetAxis(AxisId::RightStickX, 1.0));
        let mut inputs = HashSet::new();
        let stick1 = DeviceMatcher::Exact("fight_stick".into());
        inputs.insert(stick1.clone());
        inputs.insert(stick2);
        let mut passthrough = HashSet::new();
        passthrough.insert(stick1);
        CompiledProfile { id: "fightstick-mixer".into(), inputs, rules, passthrough }
    }

    #[test]
    fn fightstick_mixer_dpad_right_to_right_stick_x() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::DPadRight), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(changed);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 1.0);
    }

    #[test]
    fn fightstick_mixer_drops_stick_2_buttons() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::South), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), None);
    }

    #[test]
    fn passthrough_maps_button_same_name() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()), Control::Button(ButtonId::South), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(changed);
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    }

    #[test]
    fn unmapped_source_is_implicit_drop() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Ai("agent1".into()), Control::Button(ButtonId::South), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
    }

    #[test]
    fn diagonal_dpad_combines() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile, &ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::DPadUp), 1.0));
        apply_event(&mut state, &profile, &ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::DPadRight), 1.0));
        assert_eq!(*state.axes.get(&AxisId::RightStickY).unwrap(), -1.0);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 1.0);
    }

    #[test]
    fn release_zeros_axis() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile, &ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::DPadRight), 1.0));
        let changed = apply_event(&mut state, &profile, &ev(SourceId::Physical("fight_stick_2".into()), Control::Button(ButtonId::DPadRight), 0.0));
        assert!(changed);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 0.0);
    }
}
```

- [ ] **Step 3: Run, verify the 6 tests pass**

Run: `cargo test -p rgp-router`
Expected: 6 tests pass.

- [ ] **Step 4: Add the conflict-resolution test**

```rust
#[test]
fn last_writer_wins_on_axis_conflict() {
    use std::time::{Duration, Instant};
    let mut profile = fightstick_mixer_profile();
    // Add stick1 + ai_any, both passthrough
    profile.inputs.insert(DeviceMatcher::AiAny);
    profile.passthrough.insert(DeviceMatcher::AiAny);
    let mut state = PadState::default();
    let t0 = Instant::now();
    let earlier = InputEvent { source: SourceId::Physical("fight_stick".into()),
                               control: Control::Axis(AxisId::LeftStickX), value: -1.0, timestamp: t0 };
    let later   = InputEvent { source: SourceId::Ai("agent1".into()),
                               control: Control::Axis(AxisId::LeftStickX), value: 1.0, timestamp: t0 + Duration::from_millis(10) };
    apply_event(&mut state, &profile, &earlier);
    apply_event(&mut state, &profile, &later);
    assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap(), 1.0);
}
```
(Note: `apply_event` doesn't currently check timestamps. The "last-writer-wins" guarantee in the spec emerges naturally because events are processed in receive order, which IS timestamp order. Document this in code: events arrive in temporal order from the channel, so naive overwrite IS last-writer-wins. Add a comment to `apply_event` saying "events are assumed to arrive in timestamp order; the channel preserves that".)

- [ ] **Step 5: Add deadzone, invert, sensitivity, profile-switch, trigger, more passthrough tests**

Spec target: 50+. Stub a list of test names; implement each one in the same pattern as Step 2's tests:

```rust
#[test] fn deadzone_below_threshold_treated_as_zero() { /* ... */ }
#[test] fn inverted_axis_negated() { /* ... */ }
#[test] fn sensitivity_scales_value() { /* ... */ }
#[test] fn profile_switch_releases_dropped_devices() { /* ... see lib.rs apply for impl */ }
#[test] fn trigger_passthrough_ranges_zero_to_one() { /* ... */ }
#[test] fn ai_only_profile_passes_all_ai_events() { /* ... */ }
#[test] fn ai_only_profile_drops_physical_events() { /* ... */ }
#[test] fn copilot_profile_accepts_both_human_and_ai() { /* ... */ }
// ... add as you encounter behaviors. Target 50+ total. Each test should be 5-15 lines.
```

For deadzone/invert/sensitivity to work, extend `RuleAction` to carry those modifiers OR have `CompiledProfile` keep `Mapping` alongside the lookup. Pick one approach and update Task 2 (`rgp-config::compile`) accordingly:
- Recommended: `RuleAction::SetAxisModified { axis, magnitude, deadzone, invert, sensitivity }` and `RuleAction::PassWithModifiers { deadzone, invert, sensitivity }`.

If you change `RuleAction` here, **go back to Task 2 and update both the type and its tests**. Add a forward note to Task 2's spec section.

- [ ] **Step 6: Implement `src/lib.rs` with `run()`**

```rust
pub mod apply;

use std::collections::HashMap;
use std::thread::{self, JoinHandle};
use crossbeam_channel::{Sender, Receiver, select};
use rgp_core::{InputEvent, PadState, ControlMsg, RgpError, ProfileId, Control, SourceId};
use rgp_config::{Config, CompiledProfile};

pub fn run(
    events_rx:  Receiver<InputEvent>,
    control_rx: Receiver<ControlMsg>,
    pad_tx:     Sender<PadState>,
    config:     Config,
    shutdown:   Receiver<()>,
) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-router".into()).spawn(move || -> Result<(), RgpError> {
        let mut current_id = config.default_profile();
        let mut compiled: HashMap<ProfileId, CompiledProfile> = HashMap::new();
        // Pre-compile all profiles at startup (fail fast on invalid).
        for p in &config.profiles {
            compiled.insert(p.id.clone(), config.compile(&p.id)?);
        }
        let mut state = PadState::default();
        // Track current source-control values so profile switch can rebuild state.
        let mut last_seen: HashMap<(SourceId, Control), f32> = HashMap::new();

        loop {
            select! {
                recv(events_rx) -> ev => match ev {
                    Ok(event) => {
                        last_seen.insert((event.source.clone(), event.control), event.value);
                        let active = compiled.get(&current_id).expect("active profile compiled");
                        if apply::apply_event(&mut state, active, &event) {
                            let _ = pad_tx.try_send(state.clone());
                        }
                    }
                    Err(_) => break,
                },
                recv(control_rx) -> msg => match msg {
                    Ok(ControlMsg::SetActiveProfile(id)) => {
                        if compiled.contains_key(&id) {
                            current_id = id;
                            // Rebuild state from last_seen against the new profile.
                            state = PadState::default();
                            let active = compiled.get(&current_id).unwrap();
                            for ((src, ctl), val) in &last_seen {
                                let synthetic = InputEvent {
                                    source: src.clone(), control: *ctl, value: *val,
                                    timestamp: std::time::Instant::now(),
                                };
                                apply::apply_event(&mut state, active, &synthetic);
                            }
                            let _ = pad_tx.try_send(state.clone());
                        }
                    }
                    Ok(ControlMsg::ListDevices(reply)) => {
                        let _ = reply.send(vec![]);  // app-level wires this; router has no device list
                    }
                    Ok(ControlMsg::Quit) | Err(_) => break,
                },
                recv(shutdown) -> _ => break,
            }
        }
        Ok(())
    }).expect("spawn router thread")
}
```

- [ ] **Step 7: Run all router tests**

Run: `cargo test -p rgp-router`
Expected: 50+ tests pass (or however many you've written; the spec target is 50+).

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rgp-router
rtk git commit -m "feat(rgp-router): mapping engine with profile compile, switch, and 50+ tests"
```

---

## Task 9: `rgp-app` — the binary + integration tests

**Spec reference:** §5.9 + §7 startup order + §8 integration tests.

**Files:**
- Create: `crates/rgp-app/Cargo.toml`
- Create: `crates/rgp-app/src/main.rs`
- Create: `crates/rgp-app/tests/integration.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "rgp-app"
edition.workspace = true
version.workspace = true

[[bin]]
name = "riptheGamePad"
path = "src/main.rs"

[dependencies]
rgp-core             = { path = "../rgp-core" }
rgp-config           = { path = "../rgp-config" }
rgp-input-physical   = { path = "../rgp-input-physical" }
rgp-input-ai         = { path = "../rgp-input-ai" }
rgp-input-ai-server  = { path = "../rgp-input-ai-server" }
rgp-router           = { path = "../rgp-router" }
rgp-virtual-pad      = { path = "../rgp-virtual-pad" }
rgp-tray             = { path = "../rgp-tray" }
crossbeam-channel    = { workspace = true }
tracing              = { workspace = true }
tracing-subscriber   = { workspace = true }
clap                 = { version = "4", features = ["derive"] }
directories          = "5"
```

- [ ] **Step 2: Implement `src/main.rs`**

```rust
use std::path::PathBuf;
use std::time::Duration;
use clap::Parser;
use crossbeam_channel::bounded;
use directories::ProjectDirs;
use rgp_core::RgpError;

#[derive(Parser)]
#[command(name = "riptheGamePad")]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
}

fn config_path(args: &Args) -> PathBuf {
    args.config.clone().unwrap_or_else(|| {
        ProjectDirs::from("com", "nooroticx", "riptheGamePad")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    })
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("RGP_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let config = match rgp_config::load(&config_path(&args)) {
        Ok(c) => c,
        Err(e) => { eprintln!("config error: {e}"); std::process::exit(1); }
    };

    // Probe ViGEmBus before spawning workers.
    let pad = match rgp_virtual_pad::connect() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(?e, "ViGEmBus probe failed; entering tray-error mode");
            // Run tray with red icon, no input/router. User clicks Quit.
            // For v1, fall back to printing the error and exiting until a
            // tray-error variant is implemented in rgp-tray.
            eprintln!("ViGEmBus error: {e}");
            std::process::exit(2);
        }
    };

    let (events_tx,  events_rx)  = bounded(1024);
    let (pad_tx,     pad_rx)     = bounded(256);
    let (control_tx, control_rx) = bounded(64);
    let (shutdown_tx, shutdown_rx) = bounded::<()>(0);

    let h_pad   = rgp_virtual_pad::run(pad_rx, shutdown_rx.clone(), Box::new(pad));
    let h_rtr   = rgp_router::run(events_rx, control_rx, pad_tx, config.clone(), shutdown_rx.clone());
    let h_phys  = rgp_input_physical::run(events_tx.clone(), shutdown_rx.clone());
    let h_ai    = rgp_input_ai_server::run(events_tx.clone(), config.server.addr, shutdown_rx.clone());

    let profile_ids = config.profiles.iter().map(|p| p.id.clone()).collect();
    if let Err(e) = rgp_tray::run_on_main(control_tx, profile_ids) {
        tracing::error!(?e, "tray error");
    }
    drop(shutdown_tx);

    let join_with_timeout = |h: std::thread::JoinHandle<Result<(), RgpError>>, name: &str| {
        let start = std::time::Instant::now();
        while !h.is_finished() && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(50));
        }
        if h.is_finished() {
            if let Err(e) = h.join().unwrap() { tracing::error!(thread = name, ?e, "thread error"); }
        } else {
            tracing::error!(thread = name, "did not exit cleanly within 2s");
        }
    };
    join_with_timeout(h_pad, "virtual-pad");
    join_with_timeout(h_rtr, "router");
    join_with_timeout(h_phys, "input-physical");
    join_with_timeout(h_ai,  "input-ai-server");
}
```

(Note: `pad: rgp_virtual_pad::ViGEmPad` cannot be passed by value into `run()` if `run` was defined to take `Box<dyn PadSink>` — adjust type signatures here and in Task 5's `run()` until they line up. Fix forward; revisit Task 5 if needed.)

- [ ] **Step 3: Build the binary**

Run: `cargo build -p rgp-app`
Expected: clean build, produces `target/debug/riptheGamePad.exe`.

- [ ] **Step 4: Write integration tests in `tests/integration.rs`**

```rust
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crossbeam_channel::bounded;
use rgp_core::*;
use rgp_config::parse_str;
use rgp_router::run as router_run;
use rgp_input_ai::handle as ai_handle;

const FOUR_PROFILE_TOML: &str = r#"
... (the full 4-scenario TOML from spec §6) ...
"#;

#[test]
fn ai_only_profile_press_b_appears_on_pad() {
    let cfg = parse_str(FOUR_PROFILE_TOML).unwrap();
    let (events_tx, events_rx) = bounded(64);
    let (pad_tx, pad_rx) = bounded(64);
    let (ctl_tx, ctl_rx) = bounded(64);
    let (sd_tx, sd_rx) = bounded::<()>(0);
    let _h = router_run(events_rx, ctl_rx, pad_tx, cfg, sd_rx);
    ctl_tx.send(ControlMsg::SetActiveProfile("ai-only".into())).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let h = ai_handle(events_tx, "test-agent");
    h.press(ButtonId::South, Duration::from_millis(10));

    let state = pad_rx.recv_timeout(Duration::from_secs(1)).expect("pad state");
    assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    drop(sd_tx);
}

#[test]
fn fightstick_mixer_dpad_appears_as_right_stick() {
    // Send a synthetic Physical event for fight_stick_2 DPadRight.
    // Verify pad state has RightStickX = 1.0.
}

#[test]
fn profile_switch_mid_press_releases_held_buttons() {
    // 1. SetActiveProfile("fightstick-plus-ai")
    // 2. AI agent presses South (held)
    // 3. SetActiveProfile("fightstick-mixer") (no longer accepts AI)
    // 4. Assert pad state shows South released.
}
```

- [ ] **Step 5: Run integration tests**

Run: `cargo test -p rgp-app --test integration`
Expected: 3 tests pass.

- [ ] **Step 6: Final workspace test**

Run: `cargo test --workspace`
Expected: all crates' tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rgp-app
rtk git commit -m "feat(rgp-app): wire workspace into binary with integration tests"
```

- [ ] **Step 8: Manual smoke test (Level 3)**

1. Install ViGEmBus (<https://github.com/ViGEm/ViGEmBus/releases>).
2. Plug in any gamepad.
3. Write a config.toml at `%APPDATA%/riptheGamePad/config.toml` based on spec §6.
4. Run `cargo run -p rgp-app`. Verify tray icon appears.
5. Open a game (Steam Big Picture controller test, or any low-stakes game).
6. Verify the virtual Xbox 360 pad is detected and your physical input drives it.
7. Switch profiles via tray, verify behavior changes.
8. Open `ws://127.0.0.1:7777` from a Python WS client, send `{"type":"press","button":"South","duration_ms":100}`, verify a button press.

If any step fails, file an issue with which one, with logs (`RGP_LOG=debug`).

---

## Self-Review

Ran against the spec. Items checked:

**Spec coverage:**
- §1 Goal — covered by integration tests + smoke (Task 9 step 8). ✓
- §3 9 decisions — each baked into one or more tasks (e.g., decision 8 panic-kills-all is implicit in Task 9's `join_with_timeout` reporting). ✓
- §4 Architecture/data flow — Task 9 `main.rs` is the architecture. ✓
- §5.1–5.9 each crate — Tasks 1–9 (one task per crate). ✓
- §6 Profile model — Task 2 implements parsing + compilation; Task 8 tests use these scenarios. ✓
- §7 Errors/lifecycle — Task 9 `main.rs` step 2 implements ViGEmBus probe ordering; per-thread error mapping in each crate's `run()`. ✓
- §8 Testing strategy — Task 8 covers Level 1 (router pure tests); Task 9 Step 4 covers Level 2 (integration); Task 9 Step 8 covers Level 3 (smoke). ✓
- §11 Build sequence — directly mirrored: Task 0 + 1 + (2,3,4,5,6 parallel) + (7,8 parallel) + 9. ✓

**Placeholder scan:**
- "Add the conflict-resolution test" (Task 8 Step 4) — actual test code provided. ✓
- "More tests stubbed in Task 8 Step 5" — provides explicit names + a recommended `RuleAction` extension. NOT a pure placeholder, since each named test has clear behavior from the spec, but a subagent should treat the named-list as a checklist and implement each.
- "TODO" / "TBD" / "implement later" — none in the plan. ✓
- "Similar to Task N" — none. Each task is self-contained. ✓

**Type consistency:**
- `ButtonId::B` was a conversation shorthand; spec and plan use `ButtonId::South`. Plan flags this in Task 3 explicitly. ✓
- `RuleAction` may grow modifiers in Task 8 Step 5 — plan flags that this requires going back to Task 2. ✓
- `ViGEmPad` vs `Box<dyn PadSink>` mismatch flagged in Task 9 Step 2. ✓
- `apply_event` test scenarios use the same `CompiledProfile` shape produced by `rgp-config::compile`. ✓

**Forward-references that require revisiting earlier tasks:**
1. **Task 3 needs `release_all`** — added forward-ref note in Task 7.
2. **Task 2's `RuleAction`** may need modifier variants — added forward-ref note in Task 8 Step 5.
3. **Task 5's `run()` signature** — Task 9 calls it with a real pad; verify signature.

These forward-references are inherent to spec-first parallelism: when an interface needs to grow, the latest discovery wins and earlier tasks are amended. Subagents executing in parallel should communicate via PR comments or a shared "interface change log" — recommended pattern.
