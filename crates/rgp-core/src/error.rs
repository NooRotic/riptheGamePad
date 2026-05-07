use thiserror::Error;

#[derive(Debug, Error)]
pub enum RgpError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("virtual pad: {0}")]
    VirtualPad(String),
    #[error("config: {msg}{}", line.map(|l| format!(" (line {l})")).unwrap_or_default())]
    Config { line: Option<usize>, msg: String },
    #[error("input source: {0}")]
    InputSource(String),
    #[error("channel: {0}")]
    Channel(String),
}
