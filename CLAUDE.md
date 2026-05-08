# riptheGamePad — agent guide

This file is loaded automatically by Claude Code when working inside this repo. It captures the project's current state, architecture, and conventions so that any agent (or human) joining mid-stream has enough to make good decisions immediately.

## What this is

A Windows-only Rust app that exposes a single virtual gamepad (Xbox 360 via ViGEmBus) and lets you mix inputs from multiple physical controllers and AI agents into it. Two motivating use cases:

1. **Human mixer** — combine two fight sticks, or a fight stick + standard pad. Map secondary stick's 8-way DPad to the virtual right stick to handle the 10% of games that need camera/UI input alongside the 90% the fight stick covers.
2. **AI driver** — drive the virtual pad from any agent over a local WebSocket (`ws://127.0.0.1:7777`) or zero-overhead Rust API.

Single virtual pad output. Profile switching via system tray + global hotkeys. Modifiers (`deadzone`, `invert`, `sensitivity`) per rule.

## Status

- **3 PRs merged** to `main`: #1 (v1, 9 crates), #2 (v1.1 polish), #3 (v2: multi-stick mixer + Modifiers)
- **140 tests passing** across 22 suites
- `cargo clippy --workspace -- -D warnings` clean
- AI WebSocket path verified end-to-end (Steam Big Picture sees the virtual pad and registers all input frame types)
- v2.1 polish queue ready (see "What's next" below)

## Architecture

9-crate cargo workspace. Sync core, async only inside the WebSocket server. All inter-crate communication via `crossbeam-channel`.

```
rgp-core              types, errors, DeviceMatcher (no I/O, no logic)
├── rgp-config        TOML schema + validate + compile to CompiledProfile
│                     Holds Modifiers struct (deadzone/invert/sensitivity)
├── rgp-input-physical gilrs wrapper; synthesizes xinput:N source IDs
├── rgp-input-ai      in-process Rust API for agents (timer thread for press durations)
├── rgp-input-ai-server WebSocket transport over rgp-input-ai (tokio current-thread + LocalSet)
├── rgp-router        apply_event pure function; handles profile switch + last_seen rebuild
├── rgp-virtual-pad   ViGEmBus sink with PadSink trait (FakePad for tests)
├── rgp-tray          system tray + hotkeys + ViGEm-missing error mode
└── rgp-app           binary; wires the above together
```

**Threads spawned by `rgp-app`:**
1. `rgp-virtual-pad` worker (consumes `pad_rx`, drives ViGEmBus)
2. `rgp-router` worker (consumes events + control msgs, emits PadState)
3. `rgp-input-physical` worker (gilrs polling)
4. `rgp-input-ai-server` worker (tokio runtime for WS)
5. `rgp-tray` runs on the **main thread** (Win32 OS message-pump requirement)

**Profile config lives in TOML.** Profiles list which `inputs` they consume and which `rules` map source → virtual control. `to = "passthrough"` short-hands "everything 1:1." Implicit drop is the default — unmapped controls are silently ignored. Last-writer-wins on conflicts.

## Key files

- **Spec v1** (foundational design): `docs/superpowers/specs/2026-05-07-riptheGamePad-design.md`
- **Spec v2** (mixer + modifiers): `docs/superpowers/specs/2026-05-08-phase2-design.md`
- **Plan v1**: `docs/superpowers/plans/2026-05-07-riptheGamePad-implementation.md`
- **Plan v2**: `docs/superpowers/plans/2026-05-08-phase2-mixer-modifiers.md`
- **Default config template**: `assets/config.default.toml` (embedded into the binary; written on first run if user has no config)
- **Tray icon**: `assets/icons/rip_icon.png`
- **WebSocket smoke test**: `scripts/ws-smoke.py` (Python with `websockets`; sends 15-frame test sequence)
- **PowerShell helpers**: `scripts/rgp.psm1` (12 aliases: `rgp`, `rgp-list`, `rgp-debug`, `rgp-build`, `rgp-test`, etc.)

## Common commands

```bash
cargo test --workspace                       # 140 tests
cargo clippy --workspace -- -D warnings      # must be clean
cargo build -p rgp-app                       # produces target/debug/riptheGamePad.exe
cargo run -p rgp-app -- --list-devices       # lists connected gamepads
cargo run -p rgp-app                         # runs the tray app (needs ViGEmBus installed)
```

PowerShell aliases (after `Import-Module C:\Dev\projects\riptheGamePad\scripts\rgp.psm1`):
```
rgp           run the app
rgp-list      list devices (xinput:N or uuid:...)
rgp-debug     run with RGP_LOG=debug
rgp-build     debug build
rgp-test      cargo test --workspace
rgp-clippy    cargo clippy --workspace -- -D warnings
rgp-kill      stop running tray instance
rgp-reset     delete user config (force re-creation)
rgp-config    open user config in editor
rgp-where     print user config path
rgp-bak       list .v1.bak migration backups
rgp-ws-smoke  run scripts/ws-smoke.py
```

## Conventions and decisions worth knowing

- **`xinput:N`** identifies XInput slot N (0-3). XInput on Windows is slot-based, not device-UUID-based — gilrs returns all-zero UUIDs for any XInput device, so we synthesize `xinput:0`, `xinput:1`, etc.
- **`uuid:...`** identifies non-XInput devices (rare on Windows, common on Linux/macOS once cross-platform lands).
- **`ai:client_id`** identifies an AI agent over the WebSocket. `ai:*` is the wildcard.
- **`xinput:*`** is the wildcard matcher for any XInput slot.
- **Pure-function cores carry the test value.** `apply_event`, `pad_state_to_xgamepad`, `translate_event_type`, `Modifiers::apply`, `synthesize_source_id` are all pure and 100% covered by unit tests. Threads, I/O, and ViGEmBus are integration-tested via `FakePad` and a `Harness`.
- **gilrs init-race mitigation** (in `list_connected`): poll up to 500ms in 50ms cycles before enumerating, exit early on first device found. gilrs's Windows backend starts a background thread that races with `gamepads()` enumeration. Required for correctness — don't remove.
- **v1 config auto-migration**: any `uuid:00000000-0000-0000-0000-000000000000` literal in the user's config gets text-substituted to `xinput:0` on startup, with a `.v1.bak` backup. Idempotent.
- **Modifiers on button-source rules are rejected at config-load** (modifiers don't apply to binary inputs). Modifiers on wildcard rules are no-op for button events at runtime but applied to axes/triggers.
- **No backwards compat shims for v1 → v2**: the auto-migration handles the one ambiguous case (all-zeros UUID); other stale UUIDs are the user's to fix manually.

## Spec-first / subagent-driven workflow

This project was built using `superpowers:brainstorming` → `superpowers:writing-plans` → `superpowers:subagent-driven-development`. Pattern proven across v1, v1.1, v2:

1. Spec everything before any code (`docs/superpowers/specs/`)
2. Plan TDD-style steps per task (`docs/superpowers/plans/`)
3. Dispatch fresh subagent per task with full task text in the prompt (don't ask them to read the plan file)
4. Two-stage review per task: spec compliance, then code quality
5. Final cross-crate review at the end catches integration gaps that per-task reviews miss
6. Smoke test on real hardware before merge

Use the same flow for v2.1 and beyond. The brainstorming and writing-plans skills are the entry points.

## What's next

### v2.1 polish queue (no blockers; do in one PR)

1. Add `synthesize_source_id_gilrs_gamepad_id_maps_to_slot_integer` test — currently the assumption that `usize::from(GamepadId)` returns the XInput slot is unverified. Spec §9 risk #1.
2. Decide and act on `RuleAction::PassControlSameName` — defined but never emitted by `compile()`. Either wire it or delete the variant + its router handling.
3. Document and test the `invert + magnitude = -1.0` double-flip in `rgp-router::apply::apply_action`. Two negations cancel; user gets un-inverted output. Add comment + one test.
4. `XInputGetCapabilities` runtime probe on startup. Verify `GamepadId(N) == XInput slot N`; log a `tracing::warn!` if mismatch. Diagnostic only, don't block startup.
5. (Optional) Replace `Modifiers::is_default` exact-float equality with `Option<f32>` carriers all the way through. Cosmetic; prevents future bugs if Modifiers ever takes computed values.
6. (Optional) Dedup `parse_button` / `parse_axis` / `parse_trigger` between `rgp-config::compile` and `rgp-input-ai-server::connection`. Three call sites → one source of truth.

### Phase 3 candidates

- **Per-game profile auto-switching** — Win32 `GetForegroundWindow` + module name → `SetActiveProfile`. Killer UX feature. ~2 sessions.
- **DualShock 4 / DualSense output** — vigem-client supports it; add `[output] type = "ds4"` switch. ~half session.
- **Macro / combo recording** — capture sequence with timings, save to TOML, bind to button/hotkey. ~1.5 sessions.
- **Multiple virtual pads** — `PadId` everywhere; profiles target output pad. ~1 session.
- **Full GUI for remapping** — `egui` window. ~2 sessions.
- **Cross-platform** — Linux uinput, macOS via IOKit. Touches every input/output crate.

### Out of scope (don't propose without strong reason)

- DirectInput / non-XInput sticks on Windows (gilrs is XInput-only)
- TOML hot-reload (must restart to pick up edits)
- Anti-cheat compatibility work (kernel anti-cheat blocks virtual pads; not our problem to solve)
- Per-control modifier scoping different from per-rule (current spec is intentional)
