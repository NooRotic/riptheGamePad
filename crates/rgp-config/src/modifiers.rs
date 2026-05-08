use crate::schema::Mapping;

#[derive(Debug, Clone, Copy)]
pub struct Modifiers {
    pub deadzone: f32,
    pub invert: bool,
    pub sensitivity: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers { deadzone: 0.0, invert: false, sensitivity: 1.0 }
    }
}

impl Modifiers {
    pub fn from_mapping(m: &Mapping) -> Self {
        Modifiers {
            deadzone: m.deadzone.unwrap_or(0.0),
            invert: m.invert,
            sensitivity: m.sensitivity.unwrap_or(1.0),
        }
    }

    pub fn is_default(&self) -> bool {
        self.deadzone == 0.0 && !self.invert && self.sensitivity == 1.0
    }

    /// Apply modifiers to an axis or trigger value.
    /// Threshold-style deadzone (|v| < deadzone → 0), then optional sign flip,
    /// then multiply by sensitivity.
    pub fn apply(&self, mut v: f32) -> f32 {
        if v.abs() < self.deadzone {
            return 0.0;
        }
        if self.invert {
            v = -v;
        }
        v * self.sensitivity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_noop_on_axis_value() {
        let m = Modifiers::default();
        assert_eq!(m.apply(0.5), 0.5);
        assert_eq!(m.apply(-0.7), -0.7);
        assert_eq!(m.apply(1.0), 1.0);
        assert_eq!(m.apply(0.0), 0.0);
    }

    #[test]
    fn deadzone_zeroes_values_below_threshold() {
        let m = Modifiers { deadzone: 0.1, ..Modifiers::default() };
        assert_eq!(m.apply(0.05), 0.0);
        assert_eq!(m.apply(-0.05), 0.0);
        assert_eq!(m.apply(0.5), 0.5);
        assert_eq!(m.apply(-0.5), -0.5);
    }

    #[test]
    fn invert_flips_sign() {
        let m = Modifiers { invert: true, ..Modifiers::default() };
        assert_eq!(m.apply(0.5), -0.5);
        assert_eq!(m.apply(-0.7), 0.7);
        assert_eq!(m.apply(0.0), 0.0);
    }

    #[test]
    fn sensitivity_scales() {
        let m = Modifiers { sensitivity: 0.7, ..Modifiers::default() };
        assert!((m.apply(1.0) - 0.7).abs() < 1e-6);
        assert!((m.apply(-1.0) - -0.7).abs() < 1e-6);
    }

    #[test]
    fn combined_modifiers_compose() {
        let m = Modifiers { deadzone: 0.1, invert: true, sensitivity: 2.0 };
        assert_eq!(m.apply(0.05), 0.0);
        assert!((m.apply(0.5) - -1.0).abs() < 1e-6);
    }

    #[test]
    fn is_default_reports_correctly() {
        assert!(Modifiers::default().is_default());
        assert!(!Modifiers { deadzone: 0.1, ..Modifiers::default() }.is_default());
        assert!(!Modifiers { invert: true, ..Modifiers::default() }.is_default());
        assert!(!Modifiers { sensitivity: 0.5, ..Modifiers::default() }.is_default());
    }

    #[test]
    fn from_mapping_pulls_optional_fields() {
        use crate::schema::{Mapping, ControlSelector, RuleTarget};
        let m = Mapping {
            from: ControlSelector { device: "d".into(), control: "*".into() },
            to: RuleTarget::Passthrough("passthrough".into()),
            deadzone: Some(0.2),
            invert: true,
            sensitivity: Some(1.5),
        };
        let mods = Modifiers::from_mapping(&m);
        assert_eq!(mods.deadzone, 0.2);
        assert!(mods.invert);
        assert_eq!(mods.sensitivity, 1.5);
    }
}
