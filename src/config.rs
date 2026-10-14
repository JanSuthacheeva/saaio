use serde::Deserialize;

/// How to launch one agent: its command line and the first prompt it gets.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Agent {
    pub command: Vec<String>,
    pub prompt: String,
}
