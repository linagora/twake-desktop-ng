pub mod vfs_trait;

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::RwLock;

use crate::models::{FileNode, FileState};
use self::vfs_trait::{VfsBackend, VfsError, FileMetadata};

pub struct InMemoryVfs {
    nodes: RwLock<HashMap<String, FileNode>>,
}

impl InMemoryVfs {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl VfsBackend for InMemoryVfs {
    async fn mount(&self, _path: &Path) -> Result<(), VfsError> {
        Ok(())
    }

    async fn unmount(&self) -> Result<(), VfsError> {
        Ok(())
    }

    async fn get_node(&self, path: &Path) -> Result<FileNode, VfsError> {
        let nodes = self.nodes.read().await;
        nodes.get(path.to_str().unwrap())
            .cloned()
            .ok_or_else(|| VfsError::NotFound(path.to_str().unwrap().to_string()))
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<FileNode>, VfsError> {
        let nodes = self.nodes.read().await;
        let prefix = path.to_str().unwrap().trim_end_matches('/');

        Ok(nodes.values()
            .filter(|n| n.path.starts_with(&format!("{}/", prefix)))
            .cloned()
            .collect())
    }

    async fn create_placeholder(
        &self,
        path: &Path,
        metadata: FileMetadata,
    ) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;
        let node = FileNode {
            id: uuid::Uuid::new_v4(),
            remote_id: None,
            path: path.to_str().unwrap().to_string(),
            state: FileState::Ghost,
            size: metadata.size,
            modified: metadata.modified,
            is_dir: metadata.is_dir,
            parent_id: None,
        };
        nodes.insert(path.to_str().unwrap().to_string(), node);
        Ok(())
    }

    async fn hydrate(&self, path: &Path) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(path.to_str().unwrap()) {
            node.state = FileState::Hydrated;
        }
        Ok(())
    }

    async fn get_state(&self, path: &Path) -> Result<FileState, VfsError> {
        let nodes = self.nodes.read().await;
        nodes.get(path.to_str().unwrap())
            .map(|n| n.state)
            .ok_or_else(|| VfsError::NotFound(path.to_str().unwrap().to_string()))
    }

    async fn set_state(&self, path: &Path, state: FileState) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(path.to_str().unwrap()) {
            node.state = state;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_node() {
        let vfs = InMemoryVfs::new();
        let path = Path::new("/test/file.txt");
        
        vfs.create_placeholder(
            path,
            FileMetadata {
                size: 1024,
                modified: "2026-01-01T00:00:00Z".to_string(),
                is_dir: false,
            },
        ).await.unwrap();
        
        let node = vfs.get_node(path).await.unwrap();
        assert_eq!(node.path, "/test/file.txt");
        assert_eq!(node.state, FileState::Ghost);
        assert_eq!(node.size, 1024);
    }

    #[tokio::test]
    async fn test_hydrate_file() {
        let vfs = InMemoryVfs::new();
        let path = Path::new("/test/file.txt");
        
        vfs.create_placeholder(
            path,
            FileMetadata {
                size: 1024,
                modified: "2026-01-01T00:00:00Z".to_string(),
                is_dir: false,
            },
        ).await.unwrap();
        
        assert_eq!(vfs.get_state(path).await.unwrap(), FileState::Ghost);
        
        vfs.hydrate(path).await.unwrap();
        
        assert_eq!(vfs.get_state(path).await.unwrap(), FileState::Hydrated);
    }

    #[tokio::test]
    async fn test_list_dir() {
        let vfs = InMemoryVfs::new();
        
        vfs.create_placeholder(
            Path::new("/test/file1.txt"),
            FileMetadata { size: 100, modified: "2026-01-01T00:00:00Z".to_string(), is_dir: false },
        ).await.unwrap();
        
        vfs.create_placeholder(
            Path::new("/test/file2.txt"),
            FileMetadata { size: 200, modified: "2026-01-01T00:00:00Z".to_string(), is_dir: false },
        ).await.unwrap();
        
        vfs.create_placeholder(
            Path::new("/other/file3.txt"),
            FileMetadata { size: 300, modified: "2026-01-01T00:00:00Z".to_string(), is_dir: false },
        ).await.unwrap();
        
        let files = vfs.list_dir(Path::new("/test")).await.unwrap();
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn test_list_dir_does_not_match_sibling_prefix() {
        let vfs = InMemoryVfs::new();
        
        vfs.create_placeholder(
            Path::new("/test/file1.txt"),
            FileMetadata { size: 100, modified: "2026-01-01T00:00:00Z".to_string(), is_dir: false },
        ).await.unwrap();
        
        vfs.create_placeholder(
            Path::new("/testing/file2.txt"),
            FileMetadata { size: 200, modified: "2026-01-01T00:00:00Z".to_string(), is_dir: false },
        ).await.unwrap();
        
        let files = vfs.list_dir(Path::new("/test")).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "/test/file1.txt");
    }

    #[tokio::test]
    async fn test_get_node_not_found() {
        let vfs = InMemoryVfs::new();
        let result = vfs.get_node(Path::new("/nonexistent")).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            VfsError::NotFound(_) => (),
            _ => panic!("Expected NotFound error"),
        }
    }
}
