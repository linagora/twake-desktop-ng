use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Ghost,
    Hydrated,
    Modified,
    Syncing,
    Conflict,
    Error,
}

impl std::fmt::Display for FileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileState::Ghost => write!(f, "ghost"),
            FileState::Hydrated => write!(f, "hydrated"),
            FileState::Modified => write!(f, "modified"),
            FileState::Syncing => write!(f, "syncing"),
            FileState::Conflict => write!(f, "conflict"),
            FileState::Error => write!(f, "error"),
        }
    }
}

impl From<&str> for FileState {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ghost" => FileState::Ghost,
            "hydrated" => FileState::Hydrated,
            "modified" => FileState::Modified,
            "syncing" => FileState::Syncing,
            "conflict" => FileState::Conflict,
            "error" => FileState::Error,
            _ => FileState::Error,
        }
    }
}
