use rgp_core::{PadState, ButtonId, AxisId, TriggerId};
use vigem_client::{XGamepad, XButtons};

pub fn pad_state_to_xgamepad(state: &PadState) -> XGamepad {
    let mut buttons_raw: u16 = 0;
    let mut set = |b: ButtonId, bit: u16| {
        if *state.buttons.get(&b).unwrap_or(&false) {
            buttons_raw |= bit;
        }
    };
    set(ButtonId::South,           XButtons::A);
    set(ButtonId::East,            XButtons::B);
    set(ButtonId::West,            XButtons::X);
    set(ButtonId::North,           XButtons::Y);
    set(ButtonId::DPadUp,          XButtons::UP);
    set(ButtonId::DPadDown,        XButtons::DOWN);
    set(ButtonId::DPadLeft,        XButtons::LEFT);
    set(ButtonId::DPadRight,       XButtons::RIGHT);
    set(ButtonId::LeftStickClick,  XButtons::LTHUMB);
    set(ButtonId::RightStickClick, XButtons::RTHUMB);
    set(ButtonId::LeftBumper,      XButtons::LB);
    set(ButtonId::RightBumper,     XButtons::RB);
    set(ButtonId::Start,           XButtons::START);
    set(ButtonId::Select,          XButtons::BACK);
    set(ButtonId::Guide,           XButtons::GUIDE);

    let axis_to_i16 = |v: f32| -> i16 {
        let v = v.clamp(-1.0, 1.0);
        (v * i16::MAX as f32).round() as i16
    };
    let trig_to_u8 = |v: f32| -> u8 {
        let v = v.clamp(0.0, 1.0);
        (v * 255.0).round() as u8
    };

    XGamepad {
        buttons: XButtons { raw: buttons_raw },
        thumb_lx: axis_to_i16(*state.axes.get(&AxisId::LeftStickX).unwrap_or(&0.0)),
        thumb_ly: axis_to_i16(*state.axes.get(&AxisId::LeftStickY).unwrap_or(&0.0)),
        thumb_rx: axis_to_i16(*state.axes.get(&AxisId::RightStickX).unwrap_or(&0.0)),
        thumb_ry: axis_to_i16(*state.axes.get(&AxisId::RightStickY).unwrap_or(&0.0)),
        left_trigger:  trig_to_u8(*state.triggers.get(&TriggerId::L2).unwrap_or(&0.0)),
        right_trigger: trig_to_u8(*state.triggers.get(&TriggerId::R2).unwrap_or(&0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn south_button_sets_a_bit() {
        let mut s = PadState::default();
        s.buttons.insert(ButtonId::South, true);
        let g = pad_state_to_xgamepad(&s);
        assert_ne!(g.buttons.raw & XButtons::A, 0);
    }

    #[test]
    fn east_button_sets_b_bit() {
        let mut s = PadState::default();
        s.buttons.insert(ButtonId::East, true);
        let g = pad_state_to_xgamepad(&s);
        assert_ne!(g.buttons.raw & XButtons::B, 0);
    }

    #[test]
    fn axis_negative_one_maps_to_negative_i16() {
        let mut s = PadState::default();
        s.axes.insert(AxisId::LeftStickX, -1.0);
        let g = pad_state_to_xgamepad(&s);
        assert!(g.thumb_lx <= -32760, "expected near i16::MIN, got {}", g.thumb_lx);
    }

    #[test]
    fn axis_positive_one_maps_to_positive_i16() {
        let mut s = PadState::default();
        s.axes.insert(AxisId::RightStickY, 1.0);
        let g = pad_state_to_xgamepad(&s);
        assert!(g.thumb_ry >= 32760, "expected near i16::MAX, got {}", g.thumb_ry);
    }

    #[test]
    fn trigger_one_maps_to_255() {
        let mut s = PadState::default();
        s.triggers.insert(TriggerId::R2, 1.0);
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.right_trigger, 255);
    }

    #[test]
    fn trigger_zero_maps_to_0() {
        let s = PadState::default();
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.left_trigger, 0);
        assert_eq!(g.right_trigger, 0);
    }

    #[test]
    fn empty_state_yields_neutral_pad() {
        let s = PadState::default();
        let g = pad_state_to_xgamepad(&s);
        assert_eq!(g.buttons.raw, 0);
        assert_eq!(g.thumb_lx, 0);
        assert_eq!(g.thumb_ly, 0);
        assert_eq!(g.thumb_rx, 0);
        assert_eq!(g.thumb_ry, 0);
        assert_eq!(g.left_trigger, 0);
        assert_eq!(g.right_trigger, 0);
    }
}
