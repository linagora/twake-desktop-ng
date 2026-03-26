use serde::{Deserialize, Serialize};
use crate::models::{FileNode, FileState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,
}

impl From<&FileNode> for FileStatus {
    fn from(node: &FileNode) -> Self {
        Self {
            path: node.path.clone(),
            state: node.state,
            size: node.size,
            modified: node.modified.clone(),
        }
    }
}
