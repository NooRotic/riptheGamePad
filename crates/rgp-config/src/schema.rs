use std::collections::HashMap;
use std::net::SocketAddr;
use serde::Deserialize;
use rgp_core::ProfileId;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub devices: HashMap<String, String>,
    #[serde(rename = "profile", default)]
    pub profiles: Vec<Profile>,
    #[serde(rename = "default")]
    pub(crate) default_section: DefaultSection,
    pub server: ServerConfig,
    pub hotkeys: HotkeyConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DefaultSection {
    pub profile: String,
}

impl Config {
    pub fn default_profile(&self) -> ProfileId {
        ProfileId(self.default_section.profile.clone())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub inputs: Vec<String>,
    #[serde(rename = "rule", default)]
    pub rules: Vec<Mapping>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Mapping {
    pub from: ControlSelector,
    pub to: RuleTarget,
    #[serde(default)]
    pub deadzone: Option<f32>,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub sensitivity: Option<f32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlSelector {
    pub device: String,
    pub control: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum RuleTarget {
    Passthrough(String),
    SetAxis { axis: String, value: f32 },
    SetButton { button: String, value: bool },
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub addr: SocketAddr,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HotkeyConfig {
    pub next_profile: String,
    pub prev_profile: String,
    pub panic_disconnect: String,
}
