use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use crate::models::file_state::FileState;

// Manual implementation for FromRow since we have complex types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub remote_id: Option<String>,
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}

impl FileNode {
    pub fn new_ghost(path: &str, is_dir: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            remote_id: None,
            path: path.to_string(),
            state: FileState::Ghost,
            size: 0,
            modified: String::new(),
            is_dir,
            parent_id: None,
        }
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
