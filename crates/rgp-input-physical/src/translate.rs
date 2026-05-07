use gilrs::{Axis, Button, EventType};
use rgp_core::{AxisId, ButtonId, Control, InputEvent, SourceId};
use std::time::Instant;

/// Translate a gilrs `EventType` into an `InputEvent`.
///
/// Returns `None` for lifecycle events (Connected/Disconnected) and for
/// unmapped buttons or axes.
///
/// Note on testability: `gilrs::ev::Code` does not expose a public constructor
/// in gilrs 0.10, so this function cannot be unit-tested directly with
/// constructed `EventType` values.  Unit tests instead call `map_gilrs_button`
/// and `map_gilrs_axis` which are pure functions with no gilrs type barriers.
pub fn translate_event_type(et: &EventType, source_id: &str) -> Option<InputEvent> {
    let (control, value) = match et {
        EventType::ButtonPressed(btn, _) => (Control::Button(map_gilrs_button(*btn)?), 1.0),
        EventType::ButtonReleased(btn, _) => (Control::Button(map_gilrs_button(*btn)?), 0.0),
        EventType::AxisChanged(axis, v, _) => (Control::Axis(map_gilrs_axis(*axis)?), *v),
        // ButtonChanged carries an analogue pressure value; we rely on
        // ButtonPressed / ButtonReleased for discrete events instead.
        EventType::ButtonChanged(_, _, _) => return None,
        // Lifecycle events; handled (Disconnected) or ignored (Connected) by lib.rs.
        EventType::Connected | EventType::Disconnected => return None,
        // Everything else (ForceFeedbackEffectCompleted, etc.).
        _ => return None,
    };
    Some(InputEvent {
        source: SourceId::Physical(source_id.to_string()),
        control,
        value,
        timestamp: Instant::now(),
    })
}

/// Map a gilrs `Button` to a `ButtonId`.
///
/// gilrs uses `LeftTrigger` / `RightTrigger` for the shoulder bumpers and
/// `LeftTrigger2` / `RightTrigger2` for the analogue triggers (L2/R2).
/// Analogue triggers also appear as `Axis::LeftZ` / `Axis::RightZ`; they are
/// not yet wired in v1 and are a known gap.
pub fn map_gilrs_button(b: Button) -> Option<ButtonId> {
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
        // gilrs LeftTrigger = shoulder bumper (LB/L1), NOT the analogue trigger.
        Button::LeftTrigger => Some(ButtonId::LeftBumper),
        Button::RightTrigger => Some(ButtonId::RightBumper),
        Button::Start => Some(ButtonId::Start),
        Button::Select => Some(ButtonId::Select),
        Button::Mode => Some(ButtonId::Guide),
        _ => None,
    }
}

/// Map a gilrs `Axis` to an `AxisId`.
///
/// `Axis::LeftZ` and `Axis::RightZ` (L2/R2 analogue triggers) are intentionally
/// unmapped in v1.
pub fn map_gilrs_axis(a: Axis) -> Option<AxisId> {
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

    // Tests call map_gilrs_button / map_gilrs_axis directly, bypassing
    // gilrs::ev::Code whose constructor is not publicly accessible in gilrs 0.10.
    // Integration coverage of translate_event_type itself lives in Task 9.

    #[test]
    fn south_button_maps_to_south() {
        let result = map_gilrs_button(Button::South).expect("South must map");
        assert_eq!(result, ButtonId::South);
    }

    #[test]
    fn all_face_buttons_map() {
        for (btn, expected) in [
            (Button::South, ButtonId::South),
            (Button::East, ButtonId::East),
            (Button::North, ButtonId::North),
            (Button::West, ButtonId::West),
        ] {
            assert_eq!(map_gilrs_button(btn), Some(expected), "{btn:?}");
        }
    }

    #[test]
    fn dpad_buttons_map() {
        for (btn, expected) in [
            (Button::DPadUp, ButtonId::DPadUp),
            (Button::DPadDown, ButtonId::DPadDown),
            (Button::DPadLeft, ButtonId::DPadLeft),
            (Button::DPadRight, ButtonId::DPadRight),
        ] {
            assert_eq!(map_gilrs_button(btn), Some(expected), "{btn:?}");
        }
    }

    #[test]
    fn stick_clicks_and_bumpers_map() {
        assert_eq!(map_gilrs_button(Button::LeftThumb), Some(ButtonId::LeftStickClick));
        assert_eq!(map_gilrs_button(Button::RightThumb), Some(ButtonId::RightStickClick));
        assert_eq!(map_gilrs_button(Button::LeftTrigger), Some(ButtonId::LeftBumper));
        assert_eq!(map_gilrs_button(Button::RightTrigger), Some(ButtonId::RightBumper));
    }

    #[test]
    fn meta_buttons_map() {
        assert_eq!(map_gilrs_button(Button::Start), Some(ButtonId::Start));
        assert_eq!(map_gilrs_button(Button::Select), Some(ButtonId::Select));
        assert_eq!(map_gilrs_button(Button::Mode), Some(ButtonId::Guide));
    }

    #[test]
    fn unmapped_button_returns_none() {
        assert!(map_gilrs_button(Button::Unknown).is_none());
    }

    #[test]
    fn all_sticks_map() {
        for (axis, expected) in [
            (Axis::LeftStickX, AxisId::LeftStickX),
            (Axis::LeftStickY, AxisId::LeftStickY),
            (Axis::RightStickX, AxisId::RightStickX),
            (Axis::RightStickY, AxisId::RightStickY),
        ] {
            assert_eq!(map_gilrs_axis(axis), Some(expected), "{axis:?}");
        }
    }

    #[test]
    fn trigger_axes_unmapped_in_v1() {
        // Axis::LeftZ / RightZ (L2/R2 analogue) are intentionally not wired in v1.
        assert!(map_gilrs_axis(Axis::LeftZ).is_none());
        assert!(map_gilrs_axis(Axis::RightZ).is_none());
    }
}
