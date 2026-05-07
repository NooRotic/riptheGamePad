use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProfileId(pub String);

impl From<&str> for ProfileId {
    fn from(s: &str) -> Self { ProfileId(s.to_string()) }
}

impl From<String> for ProfileId {
    fn from(s: String) -> Self { ProfileId(s) }
}
