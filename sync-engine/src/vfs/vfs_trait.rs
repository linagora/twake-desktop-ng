use crate::models::{FileNode, FileState};
use std::path::Path;
use async_trait::async_trait;

#[async_trait]
pub trait VfsBackend: Send + Sync {
    async fn mount(&self, path: &Path) -> Result<(), VfsError>;
    async fn unmount(&self) -> Result<(), VfsError>;
    async fn get_node(&self, path: &Path) -> Result<FileNode, VfsError>;
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileNode>, VfsError>;
    async fn create_placeholder(
        &self,
        path: &Path,
        metadata: FileMetadata,
    ) -> Result<(), VfsError>;
    async fn hydrate(&self, path: &Path) -> Result<(), VfsError>;
    async fn get_state(&self, path: &Path) -> Result<FileState, VfsError>;
    async fn set_state(&self, path: &Path, state: FileState) -> Result<(), VfsError>;
}

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Invalid state transition: {0} -> {1}")]
    InvalidTransition(FileState, FileState),
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: String,
    pub is_dir: bool,
}
