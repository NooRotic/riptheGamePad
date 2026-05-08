# Phase 2 Design Spec — Multi-Stick Mixer + RuleAction Modifiers

- **Status:** approved 2026-05-08
- **Author:** Walter Pollard Jr. (NooRotic)
- **Project:** `C:\Dev\projects\riptheGamePad` (branched off `main` after PR #2 merge)
- **Builds on:** `docs/superpowers/specs/2026-05-07-riptheGamePad-design.md` (v1)
- **Goal:** Enable the spec's primary use case — two physical fight sticks distinguished and mixed into one virtual pad — and complete the v1-deferred `Mapping` modifier fields (`deadzone`, `invert`, `sensitivity`).

---

## 1. Motivation

v1 smoke-testing surfaced two limitations:

1. **XInput devices are slot-based, not UUID-based.** gilrs returns `uuid:00000000-0000-0000-0000-000000000000` for every XInput device on Windows because the underlying API exposes 4 slots, not stable per-device IDs. Two physical XInput fight sticks plugged in simultaneously cannot be distinguished by the v1 identifier scheme. This breaks the spec's primary "fight stick + 8-way camera stick" mixer scenario.
2. **`deadzone`, `invert`, `sensitivity` were rejected at config-load.** v1 chose to reject these `Mapping` fields explicitly rather than silently ignore them. v2 implements them properly so users can write `deadzone = 0.1` and have it apply.

Both items are additive — existing v1 configs continue to load without modification.

## 2. Non-goals

- **DirectInput device support.** v1 is XInput-only on Windows; v2 stays the same. Non-XInput sticks remain invisible until a future input-library swap (v3).
- **Per-control modifiers.** Modifiers are per-rule. A user wanting different deadzones for `LeftStickX` vs `LeftStickY` writes two separate rules.
- **Non-linear modifier curves.** Modifiers do threshold-style deadzone (`if abs(v) < deadzone: return 0`) → optional sign flip → multiply by sensitivity. No exponential, no cubic, no smoothed-deadzone re-scaling. The exact formula is in `Modifiers::apply` in §4.2.
- **Re-numbering XInput slots.** gilrs's `GamepadId` integer maps directly to XInput slot 0–3. We don't try to make the slot stable across reconnects.

## 3. Decisions

| # | Decision | Rationale |
|---|---|---|
| 1 | XInput devices identified as `xinput:N` strings (N = slot 0–3) | Mirrors existing `ai:*` convention; reuses `SourceId::Physical(String)` without type changes. |
| 2 | `xinput:*` wildcard added as `DeviceMatcher::XInputAny` | Matches existing `DeviceMatcher::AiAny` pattern. |
| 3 | Modifiers stored as `Modifiers` struct in `CompiledProfile`, applied in `apply_event`'s value pipeline | Keeps the modifier math in one place; `apply_event` stays the test-targetable pure function. |
| 4 | `CompiledProfile.passthrough` becomes `HashMap<DeviceMatcher, Modifiers>` (was `HashSet`) | Preserves per-device modifier scoping for wildcard `to = "passthrough"` rules. |
| 5 | `CompiledProfile.rules` value becomes `(RuleAction, Modifiers)` tuple | Per-rule modifiers, not per-device. |
| 6 | Modifiers on button-source rules are rejected at config-load | Modifiers don't make sense for binary inputs (would zero out presses). |
| 7 | Modifiers on button-source events through wildcard passthrough are silently no-op at runtime | Buttons hit passthrough only when their device is in `passthrough` map; `Modifiers::apply` is called but only affects axis/trigger values. |
| 8 | Last-writer-wins on conflicting passthrough modifiers for the same device, with a `tracing::warn!` at compile time | Simpler than averaging or rejecting; matches the v1 `last_seen` event pattern. The warning surfaces the conflict in the log without blocking the load. |
| 9 | Backward compat: existing UUID-based configs continue to work | Coincidence: a user who wrote `uuid:0000-...` by hand in v1 will still match XInput slot-0 devices. |
| 10 | Modifier defaults: `Modifiers { deadzone: 0.0, invert: false, sensitivity: 1.0 }` is a no-op | Preserves v1 semantics for configs that don't specify modifiers. |

## 4. Architecture changes

### 4.1 `rgp-core`

**New `DeviceMatcher` variant:**

```rust
pub enum DeviceMatcher {
    Exact(String),
    AiAny,
    AiClient(String),
    XInputAny,                 // NEW: matches any SourceId::Physical(s) where s starts with "xinput:"
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

### 4.2 `rgp-config`

**`Modifiers` type (new file `crates/rgp-config/src/modifiers.rs`):**

```rust
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
    pub fn from_mapping(m: &super::schema::Mapping) -> Self {
        Modifiers {
            deadzone: m.deadzone.unwrap_or(0.0),
            invert: m.invert,
            sensitivity: m.sensitivity.unwrap_or(1.0),
        }
    }

    pub fn is_default(&self) -> bool {
        self.deadzone == 0.0 && !self.invert && self.sensitivity == 1.0
    }

    pub fn apply(&self, mut v: f32) -> f32 {
        if v.abs() < self.deadzone { return 0.0; }
        if self.invert { v = -v; }
        v * self.sensitivity
    }
}
```

**`CompiledProfile` shape change:**

```rust
pub struct CompiledProfile {
    pub id: ProfileId,
    pub inputs: HashSet<DeviceMatcher>,
    pub rules: HashMap<(DeviceMatcher, Control), (RuleAction, Modifiers)>,
    pub passthrough: HashMap<DeviceMatcher, Modifiers>,
}
```

**Validation rule additions:**
- `lib.rs::validate` removes the three "v1 not supported" branches for `deadzone`/`invert`/`sensitivity`.
- New rejection: any `Mapping` with non-default modifiers AND a button-source `from.control` is rejected. Error: `"modifiers (deadzone/invert/sensitivity) cannot be applied to button rule (device={...}, control={...}); buttons are binary inputs"`.
- Wildcard rules (`from.control = "*"`) with modifiers ARE allowed. The modifiers apply only to axis/trigger events at runtime; button events through the wildcard are no-op for modifiers.

**`compile.rs::Config::compile` updates:**
- Build `Modifiers::from_mapping(rule)` for each rule.
- Insert `(RuleAction, Modifiers)` tuple into `CompiledProfile.rules`.
- For passthrough rules: insert into `CompiledProfile.passthrough` map with the modifiers. If the same device already has an entry, log a tracing warning and overwrite (last-writer-wins).
- Recognize `xinput:*` as `DeviceMatcher::XInputAny` in `input_to_matcher`.

### 4.3 `rgp-input-physical`

**`list_connected` updates:**
```rust
fn synthesize_source_id(uuid_bytes: [u8; 16], gilrs_id: GamepadId) -> String {
    if uuid_bytes == [0u8; 16] {
        // XInput device — gilrs's GamepadId integer maps to XInput slot.
        format!("xinput:{}", usize::from(gilrs_id))
    } else {
        format!("uuid:{}", uuid::Uuid::from_bytes(uuid_bytes))
    }
}
```

Used by both `list_connected` and `run`'s `source_id_for` helper.

**`GamepadId` to `usize`**: gilrs 0.10's `GamepadId` implements `Into<usize>`; use `usize::from(gilrs_id)`. If a version mismatch makes that unavailable, fall back to formatting via `format!("{:?}", gilrs_id)` and parsing the integer out — but the `Into<usize>` path is canonical.

### 4.4 `rgp-router`

**`apply_event` updates:**

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

**`apply_action` and `apply_passthrough` updates:**
- Apply `modifiers.apply(value)` to axis and trigger values before storing in `PadState`.
- Button values pass through unchanged (modifiers no-op on buttons at runtime).
- For `SetAxis(axis, magnitude)` from a button-source: modifiers are no-op (button is binary; resulting axis value is `magnitude` or `0.0`, not modulated).
- For `SetAxis(axis, magnitude)` from an axis source: apply modifiers to the source axis value, then scale by `magnitude.signum()` (preserves sign convention from v1).

## 5. TOML schema (no breaking changes)

Existing `Mapping` fields `deadzone: Option<f32>`, `invert: bool`, `sensitivity: Option<f32>` are now honored at compile time (instead of rejected).

Example two-stick mixer:

```toml
[devices]
fight_stick   = "xinput:0"
fight_stick_2 = "xinput:1"

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

# (the rest of fight_stick_2's DPad rules same as v1)
```

Example with modifiers (camera-tuning use case):

```toml
[[profile]]
id = "camera-sensitivity"
name = "Camera with Custom Sensitivity"
inputs = ["xbox_pad"]

[[profile.rule]]
from = { device = "xbox_pad", control = "*" }
to = "passthrough"
deadzone = 0.05      # eliminate stick drift below 5%
sensitivity = 0.7    # 70% of normal speed

[[profile.rule]]
from = { device = "xbox_pad", control = "RightStickY" }
to = "passthrough"
invert = true        # invert Y axis only (override the wildcard above for this control)
```

(Note: the second rule overrides the first for `RightStickY` because more-specific rules win over wildcard `*`. This matches v1 lookup behavior.)

## 6. Testing strategy

### Per-crate test additions

- `rgp-core` (≈ 2 tests):
  - `device_matcher_xinput_any_matches_xinput_slots` — `xinput:0`, `xinput:1`, `xinput:9` all match.
  - `device_matcher_xinput_any_rejects_non_xinput` — `uuid:abc`, `ai:client`, `xbox_pad` all reject.

- `rgp-input-physical` (≈ 3 tests):
  - `synthesize_source_id_zero_uuid_returns_xinput_slot`
  - `synthesize_source_id_nonzero_uuid_returns_uuid_format`
  - `gilrs_gamepad_id_maps_to_slot_integer`

- `rgp-config` (≈ 5 tests):
  - `modifiers_from_mapping_with_all_fields`
  - `modifiers_default_is_noop` — `Modifiers::default().apply(0.5) == 0.5`
  - `compile_rejects_deadzone_on_button_rule`
  - `compile_rejects_invert_on_button_rule`
  - `compile_rejects_sensitivity_on_button_rule`
  - `xinput_wildcard_compiles_to_XInputAny_matcher`

- `rgp-router::apply` (≈ 12 tests):
  - `deadzone_clamps_small_axis_to_zero`
  - `deadzone_does_not_affect_above_threshold`
  - `invert_flips_axis_sign`
  - `sensitivity_scales_axis_linearly`
  - `sensitivity_clamps_at_one_after_scaling` (or doesn't clamp — decide and test)
  - `modifiers_combine_deadzone_invert_sensitivity`
  - `modifiers_apply_through_passthrough`
  - `modifiers_apply_to_trigger`
  - `modifiers_noop_on_button_passthrough`
  - `set_axis_from_axis_applies_modifiers`
  - `set_axis_from_button_ignores_modifiers`
  - `xinput_any_matches_slot_0_and_slot_1`

- `rgp-app/tests/integration.rs` (≈ 3 tests):
  - `two_stick_mixer_with_xinput_slot_aliases` — synthesize physical events with `SourceId::Physical("xinput:0")` and `SourceId::Physical("xinput:1")`, drive through router, verify mixer behavior.
  - `deadzone_applied_through_full_pipeline` — small axis value → zero on virtual pad.
  - `inverted_axis_visible_on_virtual_pad` — push up → virtual stick goes down.

**Total new tests:** ≈ 25. Workspace total expected: 104 (current) + 25 ≈ 129.

### What we don't test in v2

- Real two-physical-XInput-stick smoke (user has only one stick at the time of writing). Smoke-test plan documents this and flags the buy-second-stick checkpoint.
- DirectInput device fallback (out of scope per §2).

## 7. Build sequence (preview for writing-plans)

Roughly four sequential commits:

1. `rgp-core` — add `DeviceMatcher::XInputAny` + tests.
2. `rgp-input-physical` — `synthesize_source_id` helper + `list_connected` and `run` use it.
3. `rgp-config` — `Modifiers` struct, `CompiledProfile` shape change, `validate` updates, `compile` updates, tests for all paths.
4. `rgp-router` — `apply_event` honors modifiers, ≈ 12 new tests.
5. `rgp-app` — integration tests using XInput-slot aliases and modifiers; update `assets/config.default.toml` to mention `xinput:N` syntax.

Stages 1 and 2 are independent. Stages 3 and 4 are sequential (router depends on config). Stage 5 closes the loop.

## 8. Migration notes

- Existing v1 configs that use `deadzone`/`invert`/`sensitivity`: previously rejected at config-load with v1-not-supported error. Now accepted and applied.
- Existing v1 configs using `uuid:00000000-...` by hand to alias their single XInput stick: continue to work coincidentally — the new `synthesize_source_id` does not produce zero-UUID strings for XInput devices anymore, so this exact alias would no longer match. **One-line fix:** users update to `xinput:0`. The default config example will be updated to show this. Document in the v2 PR.
- No database, no on-disk state changes.

## 9. Risks

- **gilrs's `GamepadId` integer mapping is undocumented.** We're assuming `GamepadId(0)` maps to XInput slot 0. If gilrs assigns IDs sequentially regardless of slot (e.g., the first connected XInput device is `GamepadId(0)` even if it's plugged into slot 2), our slot identifier is unstable across plug orders. **Mitigation:** add a runtime check by querying `XInputGetCapabilities(slot)` for each slot 0–3 and matching to the connected gamepads' names. If gilrs's mapping doesn't align, document the discrepancy and consider switching to direct `XInput` calls in v2.1.
- **Modifier wildcard precedence.** A device with `to = "passthrough"` + global `deadzone = 0.05` may not behave as expected if a more-specific rule for the same control is also defined. The validation currently doesn't catch this. We accept the v1 "more-specific rule wins" semantic, document it, and provide an integration test demonstrating the override.
- **Per-control vs per-device modifier scope.** Multiple passthrough rules with conflicting modifiers for the same device → last-writer-wins in compile order. May confuse users who expect rule-line order to be irrelevant. Mitigation: tracing warning at compile time when same-device modifier conflicts are detected.

## 10. Glossary additions

- **XInput slot:** integer 0–3 corresponding to a physical USB / wireless XInput controller connection on Windows. Determined by Windows's XInput driver, not by gilrs. Stable across the lifetime of a single plug-in but not across unplug/replug cycles.
- **Modifiers:** per-rule transformations applied to axis/trigger values before they reach `PadState`. Three knobs: `deadzone` (zeroes out small inputs), `invert` (flips sign), `sensitivity` (multiplies value).

