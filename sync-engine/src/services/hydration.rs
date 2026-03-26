use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::models::FileState;
use crate::vfs::vfs_trait::VfsBackend;
use crate::db::repository::FileRepository;
use crate::vfs::vfs_trait::VfsError;

pub struct HydrationService<V: VfsBackend> {
    vfs: Arc<V>,
    repo: FileRepository,
}

impl<V: VfsBackend> HydrationService<V> {
    pub fn new(vfs: Arc<V>, repo: FileRepository) -> Self {
        Self { vfs, repo }
    }

    pub async fn hydrate_file(&self, path: &Path) -> Result<(), HydrationError> {
        info!("Hydrating file: {}", path.display());

        let node = self.vfs.get_node(path).await?;

        if node.state == FileState::Hydrated {
            info!("File already hydrated: {}", path.display());
            return Ok(());
        }

        self.vfs.set_state(path, FileState::Syncing).await?;

        if let Err(e) = self.download_file(path, &node).await {
            let _ = self.vfs.set_state(path, FileState::Error).await;
            let _ = self.repo.update_state(path.to_str().unwrap(), FileState::Error).await;
            return Err(e);
        }

        self.vfs.set_state(path, FileState::Hydrated).await?;
        self.repo.update_state(path.to_str().unwrap(), FileState::Hydrated).await?;

        info!("File hydrated: {}", path.display());
        Ok(())
    }

    async fn download_file(&self, path: &Path, _node: &crate::models::FileNode) -> Result<(), HydrationError> {
        let parent = path.parent().ok_or(HydrationError::InvalidPath)?;
        tokio::fs::create_dir_all(parent).await?;

        let mut file = File::create(path).await?;
        file.write_all(b"").await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HydrationError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("VFS error: {0}")]
    Vfs(#[from] VfsError),

    #[error("Invalid path")]
    InvalidPath,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::InMemoryVfs;
    use crate::vfs::vfs_trait::FileMetadata;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_hydrate_ghost_file() {
        let vfs = Arc::new(InMemoryVfs::new());
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repo = FileRepository { pool };
        
        let service = HydrationService::new(vfs.clone(), repo);
        
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_hydrate_file.txt");
        
        vfs.create_placeholder(
            &path,
            FileMetadata {
                size: 1024,
                modified: "2026-01-01T00:00:00Z".to_string(),
                is_dir: false,
            },
        ).await.unwrap();
        
        assert_eq!(vfs.get_state(&path).await.unwrap(), FileState::Ghost);
        
        service.hydrate_file(&path).await.unwrap();
        
        assert_eq!(vfs.get_state(&path).await.unwrap(), FileState::Hydrated);
        
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_hydrate_already_hydrated() {
        let vfs = Arc::new(InMemoryVfs::new());
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repo = FileRepository { pool };
        
        let service = HydrationService::new(vfs.clone(), repo);
        
        let path = Path::new("/test/file.txt");
        vfs.create_placeholder(
            path,
            FileMetadata {
                size: 1024,
                modified: "2026-01-01T00:00:00Z".to_string(),
                is_dir: false,
            },
        ).await.unwrap();
        
        vfs.hydrate(path).await.unwrap();
        
        let result = service.hydrate_file(path).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_hydrate_nonexistent_file() {
        let vfs = Arc::new(InMemoryVfs::new());
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repo = FileRepository { pool };
        
        let service = HydrationService::new(vfs.clone(), repo);
        
        let path = Path::new("/nonexistent/file.txt");
        let result = service.hydrate_file(path).await;
        
        assert!(result.is_err());
    }
}
