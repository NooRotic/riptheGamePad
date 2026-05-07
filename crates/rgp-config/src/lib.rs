pub mod schema;
pub mod compile;

pub use schema::*;
pub use compile::{CompiledProfile, RuleAction};

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

    // v1: reject modifier fields with a clear error so users don't silently
    // get unscaled inputs. Implement them as a follow-up; for now, surface
    // the limitation explicitly.
    for p in &cfg.profiles {
        for r in &p.rules {
            if r.deadzone.is_some() {
                return Err(RgpError::Config { line: None,
                    msg: format!("'deadzone' on rule (device={}, control={}) is not supported in v1; remove or wait for v2",
                                 r.from.device, r.from.control) });
            }
            if r.invert {
                return Err(RgpError::Config { line: None,
                    msg: format!("'invert' on rule (device={}, control={}) is not supported in v1; remove or wait for v2",
                                 r.from.device, r.from.control) });
            }
            if r.sensitivity.is_some() {
                return Err(RgpError::Config { line: None,
                    msg: format!("'sensitivity' on rule (device={}, control={}) is not supported in v1; remove or wait for v2",
                                 r.from.device, r.from.control) });
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
    cfg.devices.contains_key(input)
}
