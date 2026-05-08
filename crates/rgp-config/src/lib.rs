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

/// One-shot migration for v1 configs that referenced XInput devices via the
/// all-zeros UUID gilrs returned in v1 (`uuid:00000000-0000-0000-0000-000000000000`).
/// In v2 those devices report as `xinput:N`. This function detects the literal
/// v1 string in the config file and rewrites it to `xinput:0`, writing a
/// backup to `<path>.v1.bak` first.
///
/// No-op if the file does not exist or does not contain the v1 string.
/// Idempotent.
pub fn maybe_migrate_v1_config(path: &Path) -> Result<(), RgpError> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(RgpError::Io(e)),
    };
    const V1_ZERO_UUID: &str = "uuid:00000000-0000-0000-0000-000000000000";
    if !content.contains(V1_ZERO_UUID) {
        return Ok(());
    }
    let backup_path: std::path::PathBuf = {
        let mut p = path.as_os_str().to_owned();
        p.push(".v1.bak");
        p.into()
    };
    std::fs::write(&backup_path, &content).map_err(RgpError::Io)?;
    let migrated = content.replace(V1_ZERO_UUID, "xinput:0");
    std::fs::write(path, &migrated).map_err(RgpError::Io)?;
    tracing::info!(
        target: "rgp::config",
        path = %path.display(),
        backup = %backup_path.display(),
        "migrated v1 config: replaced all-zeros UUID with xinput:0"
    );
    Ok(())
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        let mut p = dir.clone();
        p.push(format!("rgp-migration-test-{}-{}.toml", name, std::process::id()));
        // Best-effort cleanup of any leftover from prior runs.
        let _ = std::fs::remove_file(&p);
        let bak = {
            let mut s = p.as_os_str().to_owned();
            s.push(".v1.bak");
            std::path::PathBuf::from(s)
        };
        let _ = std::fs::remove_file(&bak);
        p
    }

    #[test]
    fn no_op_if_file_missing() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        maybe_migrate_v1_config(&path).expect("should not error on missing file");
        assert!(!path.exists(), "migration should not create a file");
    }

    #[test]
    fn no_op_if_no_v1_uuid() {
        let path = temp_path("no_uuid");
        std::fs::write(&path, "fight_stick = \"xinput:0\"\n").unwrap();
        maybe_migrate_v1_config(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fight_stick = \"xinput:0\"\n");
        let bak = {
            let mut s = path.as_os_str().to_owned();
            s.push(".v1.bak");
            std::path::PathBuf::from(s)
        };
        assert!(!bak.exists(), "no backup should be written when no migration occurred");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrates_all_zeros_uuid_and_writes_backup() {
        let path = temp_path("zero_uuid");
        let original = "fight_stick = \"uuid:00000000-0000-0000-0000-000000000000\"\nother = \"value\"\n";
        std::fs::write(&path, original).unwrap();
        maybe_migrate_v1_config(&path).unwrap();
        let migrated = std::fs::read_to_string(&path).unwrap();
        assert!(migrated.contains("xinput:0"), "expected xinput:0 substitution, got: {migrated}");
        assert!(!migrated.contains("uuid:00000000"), "v1 string should be removed");
        assert!(migrated.contains("other = \"value\""), "other content preserved");
        let bak = {
            let mut s = path.as_os_str().to_owned();
            s.push(".v1.bak");
            std::path::PathBuf::from(s)
        };
        assert!(bak.exists(), "backup must be written");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), original);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
    }

    #[test]
    fn migration_is_idempotent() {
        let path = temp_path("idempotent");
        let original = "fight_stick = \"uuid:00000000-0000-0000-0000-000000000000\"\n";
        std::fs::write(&path, original).unwrap();
        maybe_migrate_v1_config(&path).unwrap();
        let after_first = std::fs::read_to_string(&path).unwrap();
        maybe_migrate_v1_config(&path).unwrap();
        let after_second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after_first, after_second);
        let bak = {
            let mut s = path.as_os_str().to_owned();
            s.push(".v1.bak");
            std::path::PathBuf::from(s)
        };
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bak);
    }
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
