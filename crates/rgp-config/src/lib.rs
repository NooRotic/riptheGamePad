pub mod schema;
pub mod compile;
pub mod modifiers;

pub use schema::*;
pub use compile::{CompiledProfile, RuleAction};
pub use modifiers::Modifiers;

use rgp_core::RgpError;
use std::path::Path;

pub fn parse_str(s: &str) -> Result<Config, RgpError> {
    let cfg: Config = toml::from_str(s)
        .map_err(|e| RgpError::Config { line: None, msg: e.to_string() })?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn load(path: &Path) -> Result<Config, RgpError> {
    let s = std::fs::read_to_string(path)?;
    parse_str(&s)
}

fn validate(cfg: &Config) -> Result<(), RgpError> {
    // Duplicate profile ids
    let mut seen = std::collections::HashSet::new();
    for p in &cfg.profiles {
        if !seen.insert(&p.id.0) {
            return Err(RgpError::Config {
                line: None,
                msg: format!("duplicate profile id: {}", p.id.0),
            });
        }
    }

    // default.profile must exist
    if !cfg.profiles.iter().any(|p| p.id.0 == cfg.default_section.profile) {
        return Err(RgpError::Config {
            line: None,
            msg: format!("default.profile '{}' not found", cfg.default_section.profile),
        });
    }

    // inputs reference real device aliases or "ai:*" or "ai:<id>"
    for p in &cfg.profiles {
        for inp in &p.inputs {
            if !is_known_input(cfg, inp) {
                return Err(RgpError::Config {
                    line: None,
                    msg: format!("unknown device alias: {inp}"),
                });
            }
        }
    }

    // rule device references must be known inputs or ai matchers
    for p in &cfg.profiles {
        for r in &p.rules {
            let dev = &r.from.device;
            if !is_known_input(cfg, dev) {
                return Err(RgpError::Config {
                    line: None,
                    msg: format!("unknown device alias in rule: {dev}"),
                });
            }
        }
    }

    // Wildcard 'control = "*"' is only valid with 'to = "passthrough"'.
    // Pairing it with SetAxis/SetButton has no defined semantics — reject early.
    for p in &cfg.profiles {
        for r in &p.rules {
            if r.from.control == "*" {
                match &r.to {
                    RuleTarget::Passthrough(_) => {} // OK
                    RuleTarget::SetAxis { .. } | RuleTarget::SetButton { .. } => {
                        return Err(RgpError::Config { line: None,
                            msg: format!(
                                "wildcard control '*' on rule (device={}) cannot be paired with SetAxis/SetButton; use a specific control name or change 'to' to \"passthrough\"",
                                r.from.device)
                        });
                    }
                }
            }
        }
    }

    // rule control names parse (skip wildcard)
    for p in &cfg.profiles {
        for r in &p.rules {
            if r.from.control != "*" {
                compile::parse_control(&r.from.control)
                    .map_err(|e| RgpError::Config { line: None, msg: e })?;
            }
        }
    }

    // RuleTarget::Passthrough must contain "passthrough"
    for p in &cfg.profiles {
        for r in &p.rules {
            if let RuleTarget::Passthrough(s) = &r.to {
                if s != "passthrough" {
                    return Err(RgpError::Config {
                        line: None,
                        msg: format!("invalid 'to' string: {s}"),
                    });
                }
            }
        }
    }

    // Modifiers (deadzone/invert/sensitivity) only apply to axes and triggers.
    // Reject them on rules whose source control is a specific button.
    // Wildcard rules (control = "*") are allowed — modifiers no-op for button
    // events at runtime.
    for p in &cfg.profiles {
        for r in &p.rules {
            let mods = crate::modifiers::Modifiers::from_mapping(r);
            if mods.is_default() {
                continue;
            }
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

    Ok(())
}

fn is_known_input(cfg: &Config, input: &str) -> bool {
    if input == "ai:*" {
        return true;
    }
    if input.starts_with("ai:") {
        return true;
    }
    if input == "xinput:*" {
        return true;
    }
    if input.starts_with("xinput:") {
        return true;
    }
    cfg.devices.contains_key(input)
}
