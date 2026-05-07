use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Frame {
    Hello { client_id: String },
    Press { button: String, duration_ms: u64 },
    Release { button: String },
    Axis { axis: String, value: f32 },
    Trigger { trigger: String, value: f32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_press() {
        let f: Frame = serde_json::from_str(r#"{"type":"press","button":"South","duration_ms":50}"#).unwrap();
        assert_eq!(f, Frame::Press { button: "South".into(), duration_ms: 50 });
    }

    #[test]
    fn parse_release() {
        let f: Frame = serde_json::from_str(r#"{"type":"release","button":"South"}"#).unwrap();
        assert_eq!(f, Frame::Release { button: "South".into() });
    }

    #[test]
    fn parse_axis() {
        let f: Frame = serde_json::from_str(r#"{"type":"axis","axis":"LeftStickX","value":-0.7}"#).unwrap();
        assert!(matches!(f, Frame::Axis { ref axis, value } if axis == "LeftStickX" && (value - -0.7_f32).abs() < 1e-6));
    }

    #[test]
    fn parse_trigger() {
        let f: Frame = serde_json::from_str(r#"{"type":"trigger","trigger":"R2","value":1.0}"#).unwrap();
        assert!(matches!(f, Frame::Trigger { ref trigger, value } if trigger == "R2" && (value - 1.0_f32).abs() < 1e-6));
    }

    #[test]
    fn parse_hello() {
        let f: Frame = serde_json::from_str(r#"{"type":"hello","client_id":"agent1"}"#).unwrap();
        assert_eq!(f, Frame::Hello { client_id: "agent1".into() });
    }

    #[test]
    fn malformed_returns_error() {
        assert!(serde_json::from_str::<Frame>(r#"{"type":"nonsense"}"#).is_err());
        assert!(serde_json::from_str::<Frame>(r#"not json"#).is_err());
    }
}
