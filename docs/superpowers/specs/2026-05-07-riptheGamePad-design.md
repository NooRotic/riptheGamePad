# riptheGamePad — Design Spec

- **Status:** approved 2026-05-07
- **Author:** Walter Pollard Jr. (NooRotic)
- **Project:** `C:\Dev\projects\riptheGamePad`
- **Stack:** Rust (cargo workspace), Windows (ViGEmBus)
- **Spec methodology:** spec-first parallelism — each crate section below is a self-contained brief that a single subagent can own end-to-end.

---

## 1. Goal

Build a Windows app that exposes a single virtual gamepad to the OS while letting the user combine inputs from multiple physical controllers and AI agents in real time. The two motivating use cases are co-equal in v1:

1. **Human mixer.** Connect two fight sticks (or a fight stick + a standard pad). One profile uses the primary stick fully and consumes only the 8-way lever from the secondary stick, mapped to the virtual right stick — solving the 10% of games that need a right stick (camera, UI). Another profile passes a standard gamepad straight through. The user swaps profiles via tray menu or hotkey when they put down the stick and pick up the pad.
2. **AI driver.** Any AI agent (Rust in-process, or any language over local WebSocket) drives the same virtual pad. AI and human inputs can co-exist in a single profile (co-pilot mode).

## 2. Non-goals (v1)

- Cross-platform (Linux/macOS). Windows-only.
- Anti-cheat bypass. ViGEmBus is signed/visible; games using kernel anti-cheat (EAC, BattlEye, Vanguard) may reject virtual pads. Not our problem to solve.
- Full GUI for remapping. v1 is tray + TOML; remapping UI deferred.
- Multiple simultaneous virtual pads. One virtual Xbox 360 pad output, period.
- Hot-reload of TOML config. Restart to pick up edits in v1.

## 3. Decisions made during brainstorming

| # | Decision | Rationale |
|---|---|---|
| 1 | Cargo workspace, fine-grained crates | Maximizes subagent isolation; each crate owns one concern. |
| 2 | Both human-mixer and AI-source in v1 (vs phased) | Two real `InputSource` implementations validate the trait shape from day 1. |
| 3 | Tray app + TOML config (no full GUI v1) | Friction-free profile swap is the critical UX; remapping UI is its own project. |
| 4 | AI source is BOTH a Rust crate (in-process) AND a WebSocket server | Cross-language access without locking out zero-overhead Rust agents. |
| 5 | Approach 1: channels-everywhere, sync core, fine-grained crates | The only architecture that fully delivers spec-first parallelism. |
| 6 | Last-writer-wins on conflicting source events | Simpler than additive merge; matches "co-drive" intuition. |
| 7 | Implicit-drop default for unmapped controls | Lets "stick #2's 8-way only" be expressed with 4 affirmative rules instead of 30 negative ones. |
| 8 | Panic-kills-all (vs per-thread isolate) | Virtual pad is shared physical-world output; partial operation is worse than crash. |
| 9 | Drop oldest on backpressure | Snapshot semantics for `PadState`; latency dominates completeness. |

## 4. Architecture & data flow

### Process model

The binary `rgp-app` creates `crossbeam_channel` MPMC channels, spawns one OS thread per concern, and runs them for the process lifetime. The system tray takes over the main thread (Windows OS message-pump requirement for tray libs).

### Channels

| Channel | Producers | Consumer | Bound |
|---|---|---|---|
| `events` | `rgp-input-physical`, `rgp-input-ai`, `rgp-input-ai-server` | `rgp-router` | 1024 |
| `pad` | `rgp-router` | `rgp-virtual-pad` | 256 |
| `control` | `rgp-tray` | `rgp-router` | 64 |
| `shutdown` | `rgp-app` (close-on-drop) | all workers | 0 |

### Threads

1. **Physical input** — `rgp-input-physical` polls `gilrs` in a tight loop, pushes `InputEvent` to `events`.
2. **AI server** — `rgp-input-ai-server` runs a tokio current-thread runtime for WebSocket I/O, decodes JSON to `InputEvent`, pushes to `events`.
3. **Router** — `rgp-router` `select!`s on `events` + `control` + `shutdown`, applies the active profile's compiled mapping, emits `PadState` on `pad`.
4. **Virtual pad** — `rgp-virtual-pad` consumes `pad`, calls `vigem-client` to update the ViGEmBus virtual Xbox 360 device.
5. **Tray** — `rgp-tray` runs the OS event loop on the main thread.

### Data flow (one button press)

```
gilrs poll
  → InputEvent { source: Physical("stick2"), control: Button(DPadRight), value: 1.0, ts }
  → events (channel)
  → router applies active profile's CompiledProfile lookup
      mapping (Physical("stick2"), Button(DPadRight)) → SetAxis(RightStickX, 1.0)
  → router updates internal PadState, emits snapshot on pad (channel)
  → virtual-pad converts PadState → vigem_client::XGamepad, submits report
  → ViGEmBus → Windows games see the virtual Xbox pad
```

AI flow is identical — only `source` differs (`SourceId::Ai("client_id")`).

## 5. Crate breakdown

```
riptheGamePad/                    (cargo workspace root)
├── Cargo.toml                    [workspace]
├── crates/
│   ├── rgp-core/                 types, errors, no logic
│   ├── rgp-config/               TOML load + validate + compile
│   ├── rgp-input-physical/       gilrs wrapper
│   ├── rgp-input-ai/             in-process Rust API for agents
│   ├── rgp-input-ai-server/      WebSocket transport over rgp-input-ai
│   ├── rgp-router/               event → mapping → PadState
│   ├── rgp-virtual-pad/          PadState → ViGEmBus
│   ├── rgp-tray/                 system tray + global hotkey
│   └── rgp-app/                  binary; wires everything
└── docs/
    └── superpowers/specs/
        └── 2026-05-07-riptheGamePad-design.md   (this file)
```

### 5.1 `rgp-core`

**Purpose:** shared types and error type. No I/O, no threads, no logic.
**Dependencies:** `serde` (for over-the-wire types), `crossbeam-channel` (re-export of `Sender`/`Receiver` aliases optional), `thiserror`.

**Public types:**

```rust
pub enum SourceId { Physical(String), Ai(String) }

pub struct InputEvent {
    pub source: SourceId,
    pub control: Control,
    pub value: f32,           // buttons: 0.0/1.0; axes: -1.0..=1.0; triggers: 0.0..=1.0
    pub timestamp: Instant,
}

pub enum Control {
    Button(ButtonId),
    Axis(AxisId),
    Trigger(TriggerId),
}

pub enum ButtonId { South, East, North, West, DPadUp, DPadDown, DPadLeft, DPadRight,
                    LeftStickClick, RightStickClick, LeftBumper, RightBumper,
                    Start, Select, Guide }
pub enum AxisId   { LeftStickX, LeftStickY, RightStickX, RightStickY }
pub enum TriggerId { L2, R2 }

pub struct PadState {
    pub buttons: BTreeMap<ButtonId, bool>,
    pub axes:    BTreeMap<AxisId, f32>,
    pub triggers:BTreeMap<TriggerId, f32>,
}

pub struct ProfileId(pub String);
pub struct DeviceInfo { pub id: SourceId, pub name: String, pub connected: bool }

pub enum ControlMsg {
    SetActiveProfile(ProfileId),
    ListDevices(crossbeam_channel::Sender<Vec<DeviceInfo>>),
    Quit,
}

pub enum RgpError {
    Io(io::Error),
    VirtualPad(String),
    Config { line: Option<usize>, msg: String },
    InputSource(String),
    Channel(String),
}
```

**Tests:** `serde` round-trip for `InputEvent` (it's the WS wire format).

---

### 5.2 `rgp-config`

**Purpose:** parse TOML, validate, compile profiles into fast lookup tables.
**Dependencies:** `serde`, `toml`, `rgp-core`.

**Public API:**

```rust
pub fn load(path: &Path) -> Result<Config, RgpError>;
pub fn parse_str(s: &str) -> Result<Config, RgpError>;

pub struct Config {
    pub profiles: Vec<Profile>,
    pub default_profile: ProfileId,
    pub devices: HashMap<String, DeviceMatcher>,   // alias → matcher
    pub server: ServerConfig,                      // ai-server bind addr
    pub hotkeys: HotkeyConfig,
}

pub struct Profile { pub id: ProfileId, pub name: String,
                     pub inputs: Vec<DeviceMatcher>, pub rules: Vec<Mapping> }

pub struct Mapping {
    pub from: ControlSelector,    // device + control (with wildcards)
    pub to:   RuleTarget,         // passthrough | SetButton | SetAxis | Drop
    pub deadzone: Option<f32>,
    pub invert: bool,
    pub sensitivity: Option<f32>,
}

pub enum DeviceMatcher { Exact(String), AiAny, AiClient(String) }

impl Config {
    pub fn compile(&self, id: &ProfileId) -> Result<CompiledProfile, RgpError>;
}

pub struct CompiledProfile {
    pub id: ProfileId,
    pub inputs: HashSet<DeviceMatcher>,
    pub rules: HashMap<(DeviceMatcher, Control), RuleAction>,
    pub passthrough: HashSet<DeviceMatcher>,
}

pub enum RuleAction { PassControlSameName, SetButton(ButtonId, bool), SetAxis(AxisId, f32), Drop }
```

**Tests:**
- Round-trip the four scenario TOML strings (mixer, pad-passthrough, ai-only, copilot).
- Validation errors: unknown device alias, duplicate profile id, unknown control name.
- `compile()` produces correct lookup table for each scenario.
- All tests pass strings to `parse_str()` — no file I/O in tests.

---

### 5.3 `rgp-input-physical`

**Purpose:** read from physical gamepads via `gilrs`, emit `InputEvent`.
**Dependencies:** `gilrs`, `rgp-core`, `crossbeam-channel`.

**Public API:**

```rust
pub fn run(events_tx: Sender<InputEvent>, shutdown: Receiver<()>) -> JoinHandle<Result<()>>;
pub fn list_connected() -> Vec<DeviceInfo>;
pub fn translate(event: &gilrs::Event, source_id: &str) -> Option<InputEvent>;  // pure
```

**Behavior:**
- Polling thread: `gilrs.next_event()` in a loop with 1ms sleep when idle.
- On `gilrs::EventType::Disconnected`, emit synthetic release events for any controls the disconnecting device was holding.
- On `events_tx` full → drop event, increment `events_dropped` counter, never block.

**Tests:**
- `translate` covers every `gilrs::EventType` variant.
- Disconnect-emits-releases logic given a held set.
- Smoke: `run()` joins cleanly on shutdown signal.

---

### 5.4 `rgp-input-ai`

**Purpose:** in-process Rust API for agents to drive the virtual pad.
**Dependencies:** `rgp-core`, `crossbeam-channel`.

**Public API:**

```rust
pub struct AiInputHandle { /* tx + source_id */ }

impl AiInputHandle {
    pub fn press(&self, button: ButtonId, duration: Duration);
    pub fn release(&self, button: ButtonId);
    pub fn axis(&self, axis: AxisId, value: f32);
    pub fn trigger(&self, t: TriggerId, value: f32);
    pub fn raw(&self, event: InputEvent);
}

pub fn handle(events_tx: Sender<InputEvent>, source_id: impl Into<String>) -> AiInputHandle;
```

**Behavior:**
- `press(B, 50ms)` immediately emits a press event and schedules a release event for `Instant::now() + 50ms`. Scheduling is handled by a single dedicated timer thread per `AiInputHandle` instance, holding a min-heap of `(deadline, ReleaseEvent)`. The timer thread sleeps until the next deadline, fires release events on the same `events_tx`, then sleeps again. This avoids `thread::spawn`-per-call overhead.

**Tests:**
- `press(B, 50ms)` produces exactly two events: press at t≈0, release at t≈50ms (±10ms tolerance).
- Concurrent `press`/`release` from multiple threads stays consistent (no lost releases).

---

### 5.5 `rgp-input-ai-server`

**Purpose:** WebSocket transport — lets non-Rust agents drive the virtual pad.
**Dependencies:** `tokio` (current-thread runtime, `rt`, `net`, `time`, `sync` features), `tokio-tungstenite`, `serde_json`, `rgp-input-ai`, `rgp-core`.

**Public API:**

```rust
pub fn run(events_tx: Sender<InputEvent>, addr: SocketAddr, shutdown: Receiver<()>)
    -> JoinHandle<Result<()>>;
```

**Wire format (JSON over WebSocket):**

```json
{"type":"press",   "button":"B", "duration_ms":50}
{"type":"release", "button":"B"}
{"type":"axis",    "axis":"LeftStickX", "value":-0.7}
{"type":"trigger", "trigger":"R2", "value":1.0}
{"type":"hello",   "client_id":"agent-twitch-chat"}     // optional handshake
```

**Behavior:**
- Each WS connection gets a unique `SourceId::Ai(client_id)`. `client_id` defaults to a random UUID; clients can override via `hello`, which **must be the first frame** on a connection — `hello` sent after any other frame is rejected with WARN log and the frame is dropped (the connection stays open).
- Translates WS messages directly into `AiInputHandle` calls — the server is a thin transport layer over `rgp-input-ai`.
- On client disconnect: emit release-all-held synthetic events for that client's source id.
- Three malformed JSON frames in a row → close that connection. Single malformed frames are logged at WARN and dropped.

**Tests:**
- Frame `{"type":"press","button":"B","duration_ms":50}` triggers `AiInputHandle::press(B, 50ms)`.
- Three malformed frames close the connection.
- Disconnect emits release events for held controls.

---

### 5.6 `rgp-router` ← highest-value crate

**Purpose:** consume `InputEvent`s, apply active profile's mapping, emit `PadState`.
**Dependencies:** `rgp-core`, `rgp-config`, `crossbeam-channel`, `tracing`.

**Public API:**

```rust
pub fn run(
    events_rx:  Receiver<InputEvent>,
    control_rx: Receiver<ControlMsg>,
    pad_tx:     Sender<PadState>,
    config:     Config,
    shutdown:   Receiver<()>,
) -> JoinHandle<Result<()>>;

// The pure semantic core — heavily unit-tested.
pub fn apply_event(state: &mut PadState, profile: &CompiledProfile, event: &InputEvent) -> bool;
```

**Mapping semantics:**
1. **Implicit drop.** Any control from a profile-listed input that has no rule is silently ignored.
2. **Passthrough.** `to = "passthrough"` shorthand maps every control to its same-named virtual counterpart.
3. **Last-writer-wins** on conflicts (by event timestamp).
4. **Deadzone / invert / sensitivity** applied per-rule before storing in `PadState`.
5. **Profile switch (atomic).** On `SetActiveProfile`, the router (a) recomputes `PadState` from current source values run through the new profile's rules, (b) emits one `PadState` on `pad_tx`, (c) releases any controls that the new profile doesn't consume.

**Tests (target: 50+):**
- `fightstick_mixer_dpad_to_right_stick`
- `fightstick_mixer_drops_stick_2_buttons`
- `last_writer_wins_on_conflict`
- `profile_switch_releases_dropped_devices`
- `diagonal_dpad_combine_to_diagonal_stick`
- `passthrough_maps_all_controls`
- `deadzone_below_threshold_treated_as_zero`
- `inverted_axis_negated`
- `unmapped_control_does_not_change_state` (`apply_event` returns `false`)
- ... one per rule type and edge case.

All tests directly call `apply_event` — no threads, no channels, no I/O.

---

### 5.7 `rgp-virtual-pad`

**Purpose:** consume `PadState`, drive ViGEmBus virtual Xbox 360 pad.
**Dependencies:** `vigem-client`, `rgp-core`, `crossbeam-channel`, `tracing`.

**Public API:**

```rust
pub fn run(pad_rx: Receiver<PadState>, shutdown: Receiver<()>) -> JoinHandle<Result<()>>;

// Pure translation — unit-tested without ViGEmBus.
pub fn pad_state_to_xgamepad(state: &PadState) -> vigem_client::XGamepad;

// Trait-based seam for testing (FakePad in tests).
pub trait PadSink {
    fn submit(&mut self, report: vigem_client::XGamepad) -> Result<(), RgpError>;
}
```

**Behavior:**
- Connect to ViGEmBus on startup. If driver missing → `RgpError::VirtualPad("ViGEmBus not installed")`, app surfaces this in tray.
- On disconnect mid-run: 5 reconnect attempts with backoff (100ms, 250ms, 500ms, 1s, 2s), then bubble up via shutdown.
- On shutdown: emit one final all-zero `PadState` to release all held buttons cleanly.

**Tests:**
- `pad_state_to_xgamepad` covers every button bit, axis scaling (-1.0..1.0 → i16::MIN..i16::MAX), trigger range (0.0..1.0 → 0..255).
- `FakePad` records submitted reports — used by integration tests in `rgp-app`.

---

### 5.8 `rgp-tray`

**Purpose:** system tray icon, profile menu, global hotkey for cycling profiles.
**Dependencies:** `tray-icon`, `global-hotkey`, `rgp-core`, `crossbeam-channel`.

**Public API:**

```rust
pub fn run_on_main(control_tx: Sender<ControlMsg>, profiles: Vec<ProfileId>) -> Result<()>;
```

**Behavior:**
- Tray menu: radio list of profile names, "Show stats", "Quit".
- Global hotkeys (configurable in TOML): `next_profile` (default F9), `prev_profile` (F10), `panic_disconnect` (Ctrl+F12, releases virtual pad immediately).
- On profile selection or hotkey: send `ControlMsg::SetActiveProfile`.
- Status icon color: green = OK, yellow = some devices disconnected, red = ViGEmBus or other fatal error.

**Tests:**
- Profile-cycle math (`next`/`prev` wraparound).
- Hotkey config parsing.
- The OS event loop is verified manually.

---

### 5.9 `rgp-app`

**Purpose:** the binary. Wires crates, owns channels, manages startup/shutdown.
**Dependencies:** all sibling crates.

**Pseudocode:**

```rust
fn main() -> Result<()> {
    init_tracing();
    let config = rgp_config::load(&config_path())?;
    let (events_tx, events_rx)   = crossbeam_channel::bounded(1024);
    let (pad_tx,    pad_rx)      = crossbeam_channel::bounded(256);
    let (control_tx, control_rx) = crossbeam_channel::bounded(64);
    let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(0);

    let h_pad    = rgp_virtual_pad::run(pad_rx, shutdown_rx.clone());
    let h_router = rgp_router::run(events_rx, control_rx, pad_tx, config.clone(), shutdown_rx.clone());
    let h_phys   = rgp_input_physical::run(events_tx.clone(), shutdown_rx.clone());
    let h_ai     = rgp_input_ai_server::run(events_tx.clone(), config.server.addr, shutdown_rx.clone());

    rgp_tray::run_on_main(control_tx, config.profile_ids())?;   // blocks
    drop(shutdown_tx);

    join_with_timeout([h_pad, h_router, h_phys, h_ai], Duration::from_secs(2));
    Ok(())
}
```

**Integration tests** live here (in `tests/integration.rs`) and use a `TestHarness` that wires real router + real config + `FakePad` + programmatic AI input source, driven by a synchronous `tick()` method.

---

## 6. Profile & configuration model

### Device aliases

Physical devices identified by a stable string (gilrs UUID). Aliases live in `[devices]` in TOML. First time a new device plugs in, the tray prompts "name this device" and writes the alias.

### TOML schema (the four scenarios)

```toml
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
# All other controls from fight_stick_2 are implicitly dropped.

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
```

### Mapping rules summary

1. Implicit drop for unmapped controls.
2. `to = "passthrough"` = same-name 1:1 mapping.
3. Last-writer-wins by timestamp on conflicting writes to the same virtual control.
4. Profile switch is atomic and releases dropped-device controls.

## 7. Errors, lifecycle, observability

### Failure modes

| Failure | Where | Behavior |
|---|---|---|
| ViGEmBus not installed | `rgp-virtual-pad` startup | `rgp-virtual-pad::run()` returns `RgpError::VirtualPad("ViGEmBus not installed")` immediately. `rgp-app` catches this **before spawning input/router threads** and starts only the tray (red icon, persistent error notification with install link). App stays running until user clicks Quit so the error is visible. |
| vigem-client connect fails mid-run | `rgp-virtual-pad` | 5 reconnect attempts (backoff), then bubble via shutdown. |
| Physical device disconnect | `rgp-input-physical` | Emit release-all-held synthetic events for that device; continue. |
| Physical device reconnect | `rgp-input-physical` | gilrs handles natively; resume. |
| TOML parse/validation error at startup | `rgp-config` | Hard fail with line number; app exits 1 before any thread starts. |
| Malformed WS JSON | `rgp-input-ai-server` | Drop frame, WARN log; close conn after 3 in a row. |
| WS client disconnect | `rgp-input-ai-server` | Release-all-held synthetic events for that client's source id. |
| `events` channel full | input-side senders | Drop event, increment `events_dropped` counter, never block. |
| `pad` channel full | router | Drop oldest snapshot — `PadState` is a snapshot (idempotent). |
| Worker thread panics | caught by `rgp-app` via `JoinHandle` | Shutdown all; tray notifies; app exits 1. |

### Startup order (matters)

1. Parse CLI args, locate config (`--config` or `%APPDATA%/riptheGamePad/config.toml`).
2. `rgp_config::load()` — hard fail before any thread starts.
3. Init tracing.
4. Create channels.
5. **Probe ViGEmBus** by attempting a `rgp-virtual-pad` connect. If it fails: skip steps 6–8, jump straight to step 9 with the tray in red-error mode (no input/router threads ever start). If it succeeds, spawn the `rgp-virtual-pad` worker thread.
6. Spawn `rgp-router`.
7. Spawn `rgp-input-physical`.
8. Spawn `rgp-input-ai-server`.
9. `rgp-tray::run_on_main` — blocks on OS event loop.
10. On tray exit: drop `shutdown_tx` to fan out shutdown.
11. Join workers with 2s timeout.

### Shutdown sequence

`shutdown_tx: Sender<()>` is `bounded(0)`. Dropping it closes all receivers; workers exit their `select!` blocks. Virtual pad worker emits one final all-zero `PadState` so games don't see stuck buttons.

### Observability

- **`tracing` crate** with per-crate target (`rgp::router`, `rgp::input::physical`, ...). Default INFO; `RGP_LOG=debug` for verbose.
- **In-process metrics** via atomics: `events_total`, `events_dropped`, `pad_updates_total`, `connected_devices`.
- **Tray "Show stats"** prints them to a notification.
- INFO/WARN logs only state changes (profile switch, connect/disconnect, errors). Per-event logs are DEBUG only.

## 8. Testing strategy

### Three levels

1. **Pure unit tests, per crate.** No I/O, no threads. Targets the pure function each crate exposes (`apply_event`, `pad_state_to_xgamepad`, `translate`, `compile`, etc.).
2. **In-process integration**, in `rgp-app`'s `tests/`. Real router + real config + `FakePad` + programmatic AI input source. Synchronous `tick()`-driven harness — no channel timing.
3. **End-to-end smoke.** Real ViGEmBus, scripted WS client. Manual + one CI job on Windows runner.

### Subagent test policy

A crate is **done** when:
- (a) it compiles standalone (`cargo build -p <crate>`),
- (b) `cargo test -p <crate>` passes the listed tests in this spec,
- (c) public API matches the interface in Section 5.

Each subagent works against `rgp-core`'s types alone. They never need to wait for a sibling crate.

### Specific high-value tests called out

- **`rgp-router`**: target ≥50 mapping tests covering every rule type, the four scenarios, conflict resolution, profile-switch behavior, deadzone/invert/sensitivity, implicit-drop.
- **`rgp-config`**: round-trip the four scenario TOMLs; reject the four classes of malformed config (unknown alias, dup id, unknown control, malformed mapping).
- **`rgp-app/tests/integration.rs`**: at minimum one test per scenario from Section 6 driven via `TestHarness`, plus one for AI-via-WS, plus one for profile-switch-mid-press.

## 9. Out of scope (v1) — captured for future planning

- Cross-platform support (Linux uinput, macOS).
- Full GUI for remapping.
- Per-game profile auto-switching (foreground window detection).
- DualShock 4 / DualSense virtual pad output (Xbox 360 only in v1).
- TOML hot-reload.
- Multiple simultaneous virtual pads.
- Macro / combo recording.
- Cloud-sync of profiles.

## 10. Glossary

- **ViGEmBus**: open-source Windows kernel driver providing virtual Xbox 360 / DualShock 4 gamepads. <https://github.com/ViGEm/ViGEmBus>
- **`vigem-client`**: Rust crate for talking to ViGEmBus.
- **`gilrs`**: Rust crate for reading physical gamepads (HID + XInput).
- **`tray-icon`** + **`global-hotkey`**: Rust crates for system tray + global hotkey on Windows.
- **`crossbeam-channel`**: MPMC channel crate, used as the comms primitive for sync workers.
- **`tokio-tungstenite`**: async WebSocket library for the AI server.
- **8-way / lever**: a fight stick joystick that emits 8 discrete directions (cardinal + diagonal), not analog.
- **SOCD cleaning**: Simultaneous Opposing Cardinal Directions resolution (e.g., ←+→ pressed at once). Implicit-drop + last-writer-wins handles this naturally; explicit SOCD config is a future feature.

## 11. Build sequence (preview for writing-plans)

The implementation plan will spawn one subagent per crate in dependency order:

1. `rgp-core` (foundation — everything depends on this; must land first).
2. **In parallel** (each depends only on `rgp-core`): `rgp-config`, `rgp-input-ai`, `rgp-input-physical`, `rgp-virtual-pad`, `rgp-tray`. Up to **five subagents simultaneously**.
3. **In parallel** (each depends on stage 1+2 outputs): `rgp-input-ai-server` (needs `rgp-input-ai`), `rgp-router` (needs `rgp-config`).
4. `rgp-app` (depends on all).

Stage 2 is the parallelism payoff: five subagents can build five crates at once with zero shared code beyond `rgp-core`'s types.
