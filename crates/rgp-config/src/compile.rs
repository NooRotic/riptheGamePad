use std::collections::{HashMap, HashSet};
use rgp_core::{ProfileId, DeviceMatcher, Control, ButtonId, AxisId, TriggerId, RgpError};
use crate::modifiers::Modifiers;

#[derive(Debug, Clone)]
pub struct CompiledProfile {
    pub id: ProfileId,
    pub inputs: HashSet<DeviceMatcher>,
    pub rules: HashMap<(DeviceMatcher, Control), (RuleAction, Modifiers)>,
    pub passthrough: HashMap<DeviceMatcher, Modifiers>,
}

#[derive(Debug, Clone)]
pub enum RuleAction {
    PassControlSameName,
    SetButton(ButtonId, bool),
    SetAxis(AxisId, f32),
    Drop,
}

pub fn parse_control(s: &str) -> Result<Control, String> {
    if let Ok(b) = parse_button(s) {
        return Ok(Control::Button(b));
    }
    if let Ok(a) = parse_axis(s) {
        return Ok(Control::Axis(a));
    }
    if let Ok(t) = parse_trigger(s) {
        return Ok(Control::Trigger(t));
    }
    Err(format!("unknown control name: {s}"))
}

pub fn parse_button(s: &str) -> Result<ButtonId, String> {
    match s {
        "South" => Ok(ButtonId::South),
        "East" => Ok(ButtonId::East),
        "North" => Ok(ButtonId::North),
        "West" => Ok(ButtonId::West),
        "DPadUp" => Ok(ButtonId::DPadUp),
        "DPadDown" => Ok(ButtonId::DPadDown),
        "DPadLeft" => Ok(ButtonId::DPadLeft),
        "DPadRight" => Ok(ButtonId::DPadRight),
        "LeftStickClick" => Ok(ButtonId::LeftStickClick),
        "RightStickClick" => Ok(ButtonId::RightStickClick),
        "LeftBumper" => Ok(ButtonId::LeftBumper),
        "RightBumper" => Ok(ButtonId::RightBumper),
        "Start" => Ok(ButtonId::Start),
        "Select" => Ok(ButtonId::Select),
        "Guide" => Ok(ButtonId::Guide),
        other => Err(format!("unknown button: {other}")),
    }
}

pub fn parse_axis(s: &str) -> Result<AxisId, String> {
    match s {
        "LeftStickX" => Ok(AxisId::LeftStickX),
        "LeftStickY" => Ok(AxisId::LeftStickY),
        "RightStickX" => Ok(AxisId::RightStickX),
        "RightStickY" => Ok(AxisId::RightStickY),
        other => Err(format!("unknown axis: {other}")),
    }
}

pub fn parse_trigger(s: &str) -> Result<TriggerId, String> {
    match s {
        "L2" => Ok(TriggerId::L2),
        "R2" => Ok(TriggerId::R2),
        other => Err(format!("unknown trigger: {other}")),
    }
}

pub(crate) fn input_to_matcher(s: &str) -> DeviceMatcher {
    if s == "ai:*" {
        DeviceMatcher::AiAny
    } else if let Some(id) = s.strip_prefix("ai:") {
        DeviceMatcher::AiClient(id.into())
    } else if s == "xinput:*" {
        DeviceMatcher::XInputAny
    } else if let Some(slot) = s.strip_prefix("xinput:") {
        // Specific xinput slot — match exactly (e.g. "xinput:0")
        let _ = slot; // slot kept for clarity
        DeviceMatcher::Exact(s.into())
    } else {
        DeviceMatcher::Exact(s.into())
    }
}

/// Resolve an alias (or bare device ID) through the devices map, then build a matcher.
fn alias_to_matcher(alias: &str, devices: &std::collections::HashMap<String, String>) -> DeviceMatcher {
    let resolved = devices.get(alias).map(|s| s.as_str()).unwrap_or(alias);
    input_to_matcher(resolved)
}

impl super::schema::Config {
    pub fn compile(&self, id: &ProfileId) -> Result<CompiledProfile, RgpError> {
        let profile = self.profiles.iter().find(|p| &p.id == id)
            .ok_or_else(|| RgpError::Config { line: None,
                msg: format!("profile not found: {}", id.0) })?;
        let mut inputs = HashSet::new();
        let mut rules = HashMap::new();
        let mut passthrough: HashMap<DeviceMatcher, Modifiers> = HashMap::new();
        for input in &profile.inputs {
            inputs.insert(alias_to_matcher(input, &self.devices));
        }
        for rule in &profile.rules {
            let dev = alias_to_matcher(&rule.from.device, &self.devices);
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
