use global_hotkey::hotkey::{Code, HotKey, Modifiers};

pub fn parse(s: &str) -> Result<HotKey, String> {
    let parts: Vec<&str> = s.split('+').collect();
    let (mods, key_str) = match parts.as_slice() {
        [k] => (Modifiers::empty(), *k),
        [m, k] => {
            let mods = match *m {
                "Ctrl" => Modifiers::CONTROL,
                "Alt" => Modifiers::ALT,
                "Shift" => Modifiers::SHIFT,
                "Meta" | "Super" => Modifiers::META,
                _ => return Err(format!("unsupported modifier: {m}")),
            };
            (mods, *k)
        }
        _ => return Err(format!("unsupported hotkey form: {s}")),
    };
    let code = parse_code(key_str)?;
    Ok(HotKey::new(Some(mods), code))
}

fn parse_code(s: &str) -> Result<Code, String> {
    match s {
        "F1" => Ok(Code::F1),
        "F2" => Ok(Code::F2),
        "F3" => Ok(Code::F3),
        "F4" => Ok(Code::F4),
        "F5" => Ok(Code::F5),
        "F6" => Ok(Code::F6),
        "F7" => Ok(Code::F7),
        "F8" => Ok(Code::F8),
        "F9" => Ok(Code::F9),
        "F10" => Ok(Code::F10),
        "F11" => Ok(Code::F11),
        "F12" => Ok(Code::F12),
        "A" | "a" => Ok(Code::KeyA),
        "B" | "b" => Ok(Code::KeyB),
        _ => Err(format!("unsupported key: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_f9() {
        assert!(parse("F9").is_ok());
    }

    #[test]
    fn parses_f10() {
        assert!(parse("F10").is_ok());
    }

    #[test]
    fn parses_ctrl_f12() {
        assert!(parse("Ctrl+F12").is_ok());
    }

    #[test]
    fn parses_alt_f9() {
        assert!(parse("Alt+F9").is_ok());
    }

    #[test]
    fn rejects_garbage_modifier() {
        assert!(parse("Foo+F9").is_err());
    }

    #[test]
    fn rejects_garbage_key() {
        assert!(parse("Ctrl+ZZZ").is_err());
    }
}
