use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceId {
    Physical(String),
    Ai(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeviceMatcher {
    Exact(String),
    AiAny,
    AiClient(String),
    XInputAny,
}

impl DeviceMatcher {
    pub fn matches(&self, id: &SourceId) -> bool {
        match (self, id) {
            (DeviceMatcher::Exact(a), SourceId::Physical(b)) => a == b,
            (DeviceMatcher::AiAny, SourceId::Ai(_)) => true,
            (DeviceMatcher::AiClient(a), SourceId::Ai(b)) => a == b,
            (DeviceMatcher::XInputAny, SourceId::Physical(b)) => b.starts_with("xinput:"),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: SourceId,
    pub name: String,
    pub connected: bool,
}
