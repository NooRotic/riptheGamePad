use std::collections::BTreeMap;
use crate::event::{ButtonId, AxisId, TriggerId};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadState {
    pub buttons: BTreeMap<ButtonId, bool>,
    pub axes: BTreeMap<AxisId, f32>,
    pub triggers: BTreeMap<TriggerId, f32>,
}
