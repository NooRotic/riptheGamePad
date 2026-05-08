use rgp_core::{InputEvent, PadState, Control, DeviceMatcher};
use rgp_config::{CompiledProfile, RuleAction, Modifiers};

/// Apply a single input event against a compiled profile, mutating state.
/// Returns true if the state actually changed.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    use rgp_core::{ButtonId, AxisId, TriggerId, SourceId, ProfileId};
    use rgp_config::Modifiers;

    fn ev(source: SourceId, control: Control, value: f32) -> InputEvent {
        InputEvent { source, control, value, timestamp: Instant::now() }
    }

    fn fightstick_mixer_profile() -> CompiledProfile {
        let mut rules = HashMap::new();
        let stick2 = DeviceMatcher::Exact("fight_stick_2".into());
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadUp)),
                     (RuleAction::SetAxis(AxisId::RightStickY, -1.0), Modifiers::default()));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadDown)),
                     (RuleAction::SetAxis(AxisId::RightStickY, 1.0), Modifiers::default()));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadLeft)),
                     (RuleAction::SetAxis(AxisId::RightStickX, -1.0), Modifiers::default()));
        rules.insert((stick2.clone(), Control::Button(ButtonId::DPadRight)),
                     (RuleAction::SetAxis(AxisId::RightStickX, 1.0), Modifiers::default()));
        let stick1 = DeviceMatcher::Exact("fight_stick".into());
        let mut inputs = HashSet::new();
        inputs.insert(stick1.clone());
        inputs.insert(stick2);
        let mut passthrough = HashMap::new();
        passthrough.insert(stick1, Modifiers::default());
        CompiledProfile {
            id: ProfileId("fightstick-mixer".into()),
            inputs, rules, passthrough,
        }
    }

    fn ai_only_profile() -> CompiledProfile {
        let mut inputs = HashSet::new();
        inputs.insert(DeviceMatcher::AiAny);
        let mut passthrough = HashMap::new();
        passthrough.insert(DeviceMatcher::AiAny, Modifiers::default());
        CompiledProfile {
            id: ProfileId("ai-only".into()),
            inputs, passthrough,
            rules: HashMap::new(),
        }
    }

    fn copilot_profile() -> CompiledProfile {
        let mut inputs = HashSet::new();
        let stick = DeviceMatcher::Exact("fight_stick".into());
        inputs.insert(stick.clone());
        inputs.insert(DeviceMatcher::AiAny);
        let mut passthrough = HashMap::new();
        passthrough.insert(stick, Modifiers::default());
        passthrough.insert(DeviceMatcher::AiAny, Modifiers::default());
        CompiledProfile {
            id: ProfileId("copilot".into()),
            inputs, passthrough,
            rules: HashMap::new(),
        }
    }

    // === Fightstick-mixer scenario ===

    #[test]
    fn fightstick_mixer_dpad_right_to_right_stick_x_max() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()),
                   Control::Button(ButtonId::DPadRight), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(changed);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 1.0);
    }

    #[test]
    fn fightstick_mixer_dpad_left_to_negative_x() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()),
                   Control::Button(ButtonId::DPadLeft), 1.0);
        apply_event(&mut state, &profile, &e);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), -1.0);
    }

    #[test]
    fn fightstick_mixer_dpad_up_to_negative_y() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadUp), 1.0));
        assert_eq!(*state.axes.get(&AxisId::RightStickY).unwrap(), -1.0);
    }

    #[test]
    fn fightstick_mixer_dpad_down_to_positive_y() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadDown), 1.0));
        assert_eq!(*state.axes.get(&AxisId::RightStickY).unwrap(), 1.0);
    }

    #[test]
    fn fightstick_mixer_dpad_release_zeros_axis() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 1.0));
        let changed = apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 0.0));
        assert!(changed);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 0.0);
    }

    #[test]
    fn fightstick_mixer_drops_stick_2_face_button() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()),
                   Control::Button(ButtonId::South), 1.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
        assert!(state.buttons.get(&ButtonId::South).copied().unwrap_or(false) == false);
    }

    #[test]
    fn fightstick_mixer_drops_stick_2_left_stick_axis() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick_2".into()),
                   Control::Axis(AxisId::LeftStickX), 0.5);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
    }

    #[test]
    fn fightstick_mixer_diagonal_combines() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadUp), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 1.0));
        assert_eq!(*state.axes.get(&AxisId::RightStickY).unwrap(), -1.0);
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 1.0);
    }

    // === Passthrough ===

    #[test]
    fn passthrough_button_press() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()),
                   Control::Button(ButtonId::South), 1.0);
        assert!(apply_event(&mut state, &profile, &e));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    }

    #[test]
    fn passthrough_button_release() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Button(ButtonId::South), 1.0));
        assert!(apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Button(ButtonId::South), 0.0)));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(false));
    }

    #[test]
    fn passthrough_axis() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()),
                   Control::Axis(AxisId::LeftStickX), -0.7);
        assert!(apply_event(&mut state, &profile, &e));
        assert!((state.axes.get(&AxisId::LeftStickX).unwrap() - -0.7).abs() < 1e-6);
    }

    #[test]
    fn passthrough_trigger() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()),
                   Control::Trigger(TriggerId::R2), 0.5);
        assert!(apply_event(&mut state, &profile, &e));
        assert!((state.triggers.get(&TriggerId::R2).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn passthrough_repeated_same_value_does_not_change() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickX), 0.5));
        let changed = apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickX), 0.5));
        assert!(!changed);
    }

    // === Implicit drop ===

    #[test]
    fn unmapped_source_drops() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Ai("agent1".into()),
                   Control::Button(ButtonId::South), 1.0);
        assert!(!apply_event(&mut state, &profile, &e));
    }

    #[test]
    fn passthrough_does_not_apply_to_non_listed_source() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        // unrelated_stick is not in profile.inputs
        let e = ev(SourceId::Physical("unrelated_stick".into()),
                   Control::Button(ButtonId::South), 1.0);
        assert!(!apply_event(&mut state, &profile, &e));
    }

    // === AI-only profile ===

    #[test]
    fn ai_only_passes_button() {
        let profile = ai_only_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Ai("agent1".into()),
                   Control::Button(ButtonId::South), 1.0);
        assert!(apply_event(&mut state, &profile, &e));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    }

    #[test]
    fn ai_only_passes_any_client() {
        let profile = ai_only_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("client_A".into()),
                        Control::Axis(AxisId::LeftStickX), 0.5));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("client_B".into()),
                        Control::Button(ButtonId::East), 1.0));
        assert!((state.axes.get(&AxisId::LeftStickX).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(true));
    }

    #[test]
    fn ai_only_drops_physical_events() {
        let profile = ai_only_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("stick".into()),
                   Control::Button(ButtonId::South), 1.0);
        assert!(!apply_event(&mut state, &profile, &e));
    }

    // === Copilot profile (last-writer-wins) ===

    #[test]
    fn copilot_axis_last_writer_wins() {
        let profile = copilot_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickX), -1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("agent1".into()),
                        Control::Axis(AxisId::LeftStickX), 1.0));
        assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap(), 1.0);
    }

    #[test]
    fn copilot_human_can_override_ai() {
        let profile = copilot_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("agent1".into()),
                        Control::Axis(AxisId::LeftStickX), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickX), -1.0));
        assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap(), -1.0);
    }

    // === SetButton rule ===

    #[test]
    fn set_button_rule_press_sets_target_value() {
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::SetButton(ButtonId::East, true), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0));
        assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(true));
    }

    #[test]
    fn set_button_rule_release_clears() {
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::SetButton(ButtonId::East, true), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 0.0));
        assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(false));
    }

    // === PassControlSameName explicit rule ===

    #[test]
    fn pass_control_same_name_action_works() {
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::PassControlSameName, Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        assert!(apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0)));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
    }

    #[test]
    fn drop_action_returns_false() {
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::Drop, Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        assert!(!apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0)));
        assert!(state.buttons.is_empty());
    }

    // === Edge cases ===

    #[test]
    fn empty_state_after_no_event_is_default() {
        let profile = fightstick_mixer_profile();
        let state = PadState::default();
        assert!(state.buttons.is_empty());
        assert!(state.axes.is_empty());
        assert!(state.triggers.is_empty());
        let _ = profile;
    }

    #[test]
    fn axis_to_axis_passthrough_preserves_zero() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()),
                   Control::Axis(AxisId::LeftStickX), 0.0);
        // Writing 0.0 to an axis that doesn't exist yet: no observable state change.
        // The entry is inserted but the value is the same as the implicit zero.
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
    }

    #[test]
    fn passthrough_trigger_zero_first_write_no_change() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Physical("fight_stick".into()),
                   Control::Trigger(TriggerId::L2), 0.0);
        let changed = apply_event(&mut state, &profile, &e);
        assert!(!changed);
    }

    #[test]
    fn set_axis_release_from_default_no_change() {
        // Releasing a button whose axis was never set: axis goes 0->0, no change.
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        let changed = apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 0.0));
        assert!(!changed);
    }

    #[test]
    fn set_button_inverted_press_sets_false() {
        // SetButton(target, false): press maps to false, release maps to true.
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::SetButton(ButtonId::East, false), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0));
        // target_when_pressed = false, so press sets East to false
        assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(false));
    }

    #[test]
    fn set_button_inverted_release_sets_true() {
        // SetButton(target, false): release maps to !false = true.
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Button(ButtonId::South)),
                     (RuleAction::SetButton(ButtonId::East, false), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        // Press (sets false), then release (sets true)
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Button(ButtonId::South), 0.0));
        assert_eq!(state.buttons.get(&ButtonId::East).copied(), Some(true));
    }

    #[test]
    fn copilot_button_last_writer_wins() {
        let profile = copilot_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Button(ButtonId::South), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("agent1".into()),
                        Control::Button(ButtonId::South), 0.0));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(false));
    }

    #[test]
    fn set_axis_from_trigger_scales_linearly() {
        // Trigger halfway -> magnitude * 0.5
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Trigger(TriggerId::R2)),
                     (RuleAction::SetAxis(AxisId::RightStickX, 1.0), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Trigger(TriggerId::R2), 0.5));
        assert!((state.axes.get(&AxisId::RightStickX).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn set_axis_from_axis_preserves_sign() {
        // Axis at -0.8 with magnitude -1.0 (signum = -1) -> output = -0.8 * -1 = 0.8
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert((dev.clone(), Control::Axis(AxisId::LeftStickX)),
                     (RuleAction::SetAxis(AxisId::RightStickX, -1.0), Modifiers::default()));
        let mut inputs = HashSet::new();
        inputs.insert(dev);
        let profile = CompiledProfile {
            id: ProfileId("p".into()),
            inputs, rules, passthrough: HashMap::new(),
        };
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("d".into()),
                        Control::Axis(AxisId::LeftStickX), -0.8));
        assert!((state.axes.get(&AxisId::RightStickX).unwrap() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn repeated_button_press_no_change() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Button(ButtonId::South), 1.0));
        // Same value again — no change.
        let changed = apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Button(ButtonId::South), 1.0));
        assert!(!changed);
    }

    #[test]
    fn ai_only_trigger_passthrough() {
        let profile = ai_only_profile();
        let mut state = PadState::default();
        let e = ev(SourceId::Ai("bot".into()),
                   Control::Trigger(TriggerId::L2), 0.75);
        assert!(apply_event(&mut state, &profile, &e));
        assert!((state.triggers.get(&TriggerId::L2).unwrap() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn rule_takes_precedence_over_passthrough_for_same_device() {
        // stick2 has rules for dpad, and is NOT in passthrough.
        // For a mapped control, rule applies. For an unmapped control, drop (not passthrough).
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        // DPadRight is mapped -> applies rule, sets axis
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 1.0));
        // South is not mapped, and stick2 is not in passthrough -> drop
        let changed = apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::South), 1.0));
        assert!(!changed);
        assert!(state.buttons.get(&ButtonId::South).is_none());
    }

    #[test]
    fn multiple_axes_independent() {
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickX), 0.3));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick".into()),
                        Control::Axis(AxisId::LeftStickY), -0.6));
        assert!((state.axes.get(&AxisId::LeftStickX).unwrap() - 0.3).abs() < 1e-6);
        assert!((state.axes.get(&AxisId::LeftStickY).unwrap() - -0.6).abs() < 1e-6);
    }

    #[test]
    fn dpad_press_mid_value_not_pressed() {
        // value = 0.4 <= 0.5 so treated as released, axis goes to 0
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        // Prime the axis first
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 1.0));
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadRight), 0.4));
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), 0.0);
    }

    #[test]
    fn dpad_press_at_threshold_is_pressed() {
        // value = 0.51 > 0.5 so treated as pressed
        let profile = fightstick_mixer_profile();
        let mut state = PadState::default();
        apply_event(&mut state, &profile,
                    &ev(SourceId::Physical("fight_stick_2".into()),
                        Control::Button(ButtonId::DPadLeft), 0.51));
        assert_eq!(*state.axes.get(&AxisId::RightStickX).unwrap(), -1.0);
    }

    #[test]
    fn ai_client_specific_matcher_blocks_other_clients() {
        // Profile that accepts only ai:agent1, not ai:agent2
        let mut inputs = HashSet::new();
        inputs.insert(DeviceMatcher::AiClient("agent1".into()));
        let mut passthrough = HashMap::new();
        passthrough.insert(DeviceMatcher::AiClient("agent1".into()), Modifiers::default());
        let profile = CompiledProfile {
            id: ProfileId("agent1-only".into()),
            inputs, passthrough,
            rules: HashMap::new(),
        };
        let mut state = PadState::default();
        // agent1 passes through
        assert!(apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("agent1".into()),
                        Control::Button(ButtonId::South), 1.0)));
        // agent2 is dropped
        assert!(!apply_event(&mut state, &profile,
                    &ev(SourceId::Ai("agent2".into()),
                        Control::Button(ButtonId::East), 1.0)));
        assert!(state.buttons.get(&ButtonId::East).is_none());
    }

    // === New modifier-behavior tests ===

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
        assert!(!changed);
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
        apply_event(&mut state, &profile,
            &ev(SourceId::Physical("d".into()), Control::Axis(AxisId::LeftStickX), 0.05));
        assert_eq!(*state.axes.get(&AxisId::LeftStickX).unwrap(), 0.0);
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
        assert!((state.axes.get(&AxisId::RightStickX).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn set_axis_from_button_ignores_modifiers() {
        let mut rules = HashMap::new();
        let dev = DeviceMatcher::Exact("d".into());
        rules.insert(
            (dev.clone(), Control::Button(ButtonId::DPadRight)),
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
            &ev(SourceId::Physical("d".into()), Control::Button(ButtonId::DPadRight), 1.0));
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
        apply_event(&mut state, &profile,
            &ev(SourceId::Physical("xinput:0".into()), Control::Button(ButtonId::South), 1.0));
        assert_eq!(state.buttons.get(&ButtonId::South).copied(), Some(true));
        let changed = apply_event(&mut state, &profile,
            &ev(SourceId::Physical("xinput:1".into()), Control::Button(ButtonId::East), 1.0));
        assert!(!changed);
    }
}
