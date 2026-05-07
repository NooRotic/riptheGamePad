use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ButtonId {
    South, East, North, West,
    DPadUp, DPadDown, DPadLeft, DPadRight,
    LeftStickClick, RightStickClick,
    LeftBumper, RightBumper,
    Start, Select, Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AxisId {
    LeftStickX, LeftStickY, RightStickX, RightStickY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TriggerId { L2, R2 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    Button(ButtonId),
    Axis(AxisId),
    Trigger(TriggerId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEvent {
    pub source: crate::source::SourceId,
    pub control: Control,
    pub value: f32,
    #[serde(skip, default = "Instant::now")]
    pub timestamp: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceId;

    #[test]
    fn input_event_serde_roundtrip_via_intermediate() {
        let ev = InputEvent {
            source: SourceId::Ai("agent1".into()),
            control: Control::Button(ButtonId::South),
            value: 1.0,
            timestamp: Instant::now(),
        };
        let json = serde_json::to_string(&ev.source).unwrap();
        let back: SourceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ev.source);

        let ctl_json = serde_json::to_string(&ev.control).unwrap();
        let ctl_back: Control = serde_json::from_str(&ctl_json).unwrap();
        assert_eq!(ctl_back, ev.control);
    }

    #[test]
    fn device_matcher_matches_correctly() {
        use crate::source::DeviceMatcher;
        assert!(DeviceMatcher::Exact("stick".into()).matches(&SourceId::Physical("stick".into())));
        assert!(!DeviceMatcher::Exact("stick".into()).matches(&SourceId::Ai("stick".into())));
        assert!(DeviceMatcher::AiAny.matches(&SourceId::Ai("anyone".into())));
        assert!(!DeviceMatcher::AiAny.matches(&SourceId::Physical("p".into())));
        assert!(DeviceMatcher::AiClient("a".into()).matches(&SourceId::Ai("a".into())));
        assert!(!DeviceMatcher::AiClient("a".into()).matches(&SourceId::Ai("b".into())));
    }
}
