use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::cozy::client::{CozyClient, CozyFileResult, CozyFolderResult};
use crate::fuse::fuse_backend::TwakeFuseFs;
use crate::models::FileState;
use crate::services::deconfliction::DeconflictionService;

#[derive(Debug)]
pub enum UploadOp {
    Create {
        path: PathBuf,
        content: Vec<u8>,
        parent_remote_id: String,
    },
    Update {
        path: PathBuf,
        content: Vec<u8>,
        remote_id: String,
        rev: Option<String>,
    },
    Delete {
        path: PathBuf,
        remote_id: String,
        rev: Option<String>,
    },
    Move {
        path: PathBuf,
        remote_id: String,
        new_parent_id: String,
        new_name: Option<String>,
        rev: Option<String>,
    },
    Mkdir {
        path: PathBuf,
        parent_remote_id: String,
        name: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Queue is closed")]
    QueueClosed,
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Cozy API error: {0}")]
    CozyApi(String),
}

pub struct UploadQueue {
    sender: mpsc::Sender<UploadOp>,
    worker_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl UploadQueue {
    pub fn new(
        cozy: CozyClient,
        vfs: Arc<TwakeFuseFs>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<UploadOp>(100);
        let deconfliction = DeconflictionService::new();

        let worker_handle = tokio::spawn(async move {
            Self::worker_loop_with_vfs(receiver, cozy, Some(vfs), deconfliction).await;
        });

        Self {
            sender,
            worker_handle: Arc::new(RwLock::new(Some(worker_handle))),
        }
    }

    pub fn new_simple(
        cozy: CozyClient,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<UploadOp>(100);
        let deconfliction = DeconflictionService::new();

        let worker_handle = tokio::spawn(async move {
            Self::worker_loop_simple(receiver, cozy, deconfliction).await;
        });

        Self {
            sender,
            worker_handle: Arc::new(RwLock::new(Some(worker_handle))),
        }
    }

    async fn worker_loop_with_vfs(
        mut receiver: mpsc::Receiver<UploadOp>,
        cozy: CozyClient,
        vfs: Option<Arc<TwakeFuseFs>>,
        deconfliction: DeconflictionService,
    ) {
        info!("UploadQueue worker started");

        while let Some(op) = receiver.recv().await {
            let result = match op {
                UploadOp::Create {
                    path,
                    content,
                    parent_remote_id,
                } => {
                    Self::handle_create(&cozy, vfs.as_ref(), &deconfliction, path, content, parent_remote_id).await
                }
                UploadOp::Update {
                    path,
                    content,
                    remote_id,
                    rev,
                } => {
                    Self::handle_update(&cozy, vfs.as_ref(), &deconfliction, path, content, remote_id, rev).await
                }
                UploadOp::Delete {
                    path,
                    remote_id,
                    rev,
                } => {
                    Self::handle_delete(&cozy, vfs.as_ref(), path, remote_id, rev).await
                }
                UploadOp::Move {
                    path,
                    remote_id,
                    new_parent_id,
                    new_name,
                    rev,
                } => {
                    Self::handle_move(&cozy, vfs.as_ref(), path, remote_id, new_parent_id, new_name, rev).await
                }
                UploadOp::Mkdir {
                    path,
                    parent_remote_id,
                    name,
                } => {
                    Self::handle_mkdir(&cozy, vfs.as_ref(), path, parent_remote_id, name).await
                }
            };

            if let Err(e) = result {
                error!("Upload operation failed: {:?}", e);
            }
        }

        info!("UploadQueue worker shutting down");
    }

    async fn worker_loop_simple(
        mut receiver: mpsc::Receiver<UploadOp>,
        cozy: CozyClient,
        deconfliction: DeconflictionService,
    ) {
        info!("UploadQueue worker started (simple mode)");

        while let Some(op) = receiver.recv().await {
            let result = match op {
                UploadOp::Create {
                    path,
                    content,
                    parent_remote_id,
                } => {
                    Self::handle_create_simple(&cozy, &deconfliction, path, content, parent_remote_id).await
                }
                UploadOp::Update {
                    path,
                    content,
                    remote_id,
                    rev,
                } => {
                    Self::handle_update_simple(&cozy, &deconfliction, path, content, remote_id, rev).await
                }
                UploadOp::Delete {
                    path,
                    remote_id,
                    rev,
                } => {
                    Self::handle_delete_simple(&cozy, path, remote_id, rev).await
                }
                UploadOp::Move {
                    path,
                    remote_id,
                    new_parent_id,
                    new_name,
                    rev,
                } => {
                    Self::handle_move_simple(&cozy, path, remote_id, new_parent_id, new_name, rev).await
                }
                UploadOp::Mkdir {
                    path,
                    parent_remote_id,
                    name,
                } => {
                    Self::handle_mkdir_simple(&cozy, path, parent_remote_id, name).await
                }
            };

            if let Err(e) = result {
                error!("Upload operation failed: {:?}", e);
            }
        }

        info!("UploadQueue worker shutting down");
    }

    async fn handle_create_simple(
        cozy: &CozyClient,
        deconfliction: &DeconflictionService,
        path: PathBuf,
        content: Vec<u8>,
        parent_remote_id: String,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Uploading new file: {}", path_str);

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| QueueError::UploadFailed("Invalid filename".into()))?;

        match cozy.upload(&parent_remote_id, filename, &content).await {
            Ok(result) => {
                info!("Upload successful: {} -> {}", path_str, result.id);
                Ok(())
            }
            Err(e) => {
                error!("Cozy upload failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_update_simple(
        cozy: &CozyClient,
        deconfliction: &DeconflictionService,
        path: PathBuf,
        content: Vec<u8>,
        remote_id: String,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Updating file: {}", path_str);

        match cozy.update(&remote_id, &content, rev.as_deref()).await {
            Ok(result) => {
                info!("Update successful: {} (rev: {})", path_str, result.rev);
                Ok(())
            }
            Err(e) => {
                error!("Cozy update failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_delete_simple(
        cozy: &CozyClient,
        path: PathBuf,
        remote_id: String,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Deleting file: {}", path_str);

        match cozy.delete(&remote_id, rev.as_deref()).await {
            Ok(()) => {
                info!("Delete successful: {}", path_str);
                Ok(())
            }
            Err(e) => {
                error!("Cozy delete failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_move_simple(
        cozy: &CozyClient,
        path: PathBuf,
        remote_id: String,
        new_parent_id: String,
        new_name: Option<String>,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Moving file: {} to parent {}", path_str, new_parent_id);

        match cozy.r#move(&remote_id, &new_parent_id, new_name.as_deref(), rev.as_deref()).await {
            Ok(result) => {
                info!("Move successful: {} -> {}", path_str, result.id);
                Ok(())
            }
            Err(e) => {
                error!("Cozy move failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_mkdir_simple(
        cozy: &CozyClient,
        path: PathBuf,
        parent_remote_id: String,
        name: String,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Creating directory: {}", path_str);

        match cozy.mkdir(&parent_remote_id, &name).await {
            Ok(result) => {
                info!("Directory created: {} -> {}", path_str, result.id);
                Ok(())
            }
            Err(e) => {
                error!("Cozy mkdir failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_create(
        cozy: &CozyClient,
        vfs: Option<&Arc<TwakeFuseFs>>,
        deconfliction: &DeconflictionService,
        path: PathBuf,
        content: Vec<u8>,
        parent_remote_id: String,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Uploading new file: {}", path_str);

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| QueueError::UploadFailed("Invalid filename".into()))?;

        match cozy.upload(&parent_remote_id, filename, &content).await {
            Ok(result) => {
                info!("Upload successful: {} -> {}", path_str, result.id);
                
                if let Some(vfs) = vfs {
                    if let Some(node) = vfs.get_node_by_path(path_str).await {
                        let mut nodes = vfs.nodes_mut().await;
                        if let Some(node) = nodes.get_mut(&node) {
                            node.remote_id = Some(result.id.clone());
                            node.state = FileState::Synced;
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Cozy upload failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_update(
        cozy: &CozyClient,
        vfs: Option<&Arc<TwakeFuseFs>>,
        deconfliction: &DeconflictionService,
        path: PathBuf,
        content: Vec<u8>,
        remote_id: String,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Updating file: {}", path_str);

        match cozy.update(&remote_id, &content, rev.as_deref()).await {
            Ok(result) => {
                info!("Update successful: {} (rev: {})", path_str, result.rev);
                
                if let Some(vfs) = vfs {
                    if let Some(node) = vfs.get_node_by_path(path_str).await {
                        let mut nodes = vfs.nodes_mut().await;
                        if let Some(node) = nodes.get_mut(&node) {
                            node.state = FileState::Synced;
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Cozy update failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_delete(
        cozy: &CozyClient,
        vfs: Option<&Arc<TwakeFuseFs>>,
        path: PathBuf,
        remote_id: String,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Deleting file: {}", path_str);

        match cozy.delete(&remote_id, rev.as_deref()).await {
            Ok(()) => {
                info!("Delete successful: {}", path_str);
                
                if let Some(vfs) = vfs {
                    if let Some(inode) = vfs.get_node_by_path(path_str).await {
                        let mut nodes = vfs.nodes_mut().await;
                        nodes.remove(&inode);
                    }
                }
                
                Ok(())
            }
            Err(e) => {
                error!("Cozy delete failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_move(
        cozy: &CozyClient,
        vfs: Option<&Arc<TwakeFuseFs>>,
        path: PathBuf,
        remote_id: String,
        new_parent_id: String,
        new_name: Option<String>,
        rev: Option<String>,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Moving file: {} to parent {}", path_str, new_parent_id);

        match cozy.r#move(&remote_id, &new_parent_id, new_name.as_deref(), rev.as_deref()).await {
            Ok(result) => {
                info!("Move successful: {} -> {}", path_str, result.id);
                Ok(())
            }
            Err(e) => {
                error!("Cozy move failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    async fn handle_mkdir(
        cozy: &CozyClient,
        vfs: Option<&Arc<TwakeFuseFs>>,
        path: PathBuf,
        parent_remote_id: String,
        name: String,
    ) -> Result<(), QueueError> {
        let path_str = path.to_str().ok_or_else(|| QueueError::UploadFailed("Invalid path".into()))?;
        info!("Creating directory: {}", path_str);

        match cozy.mkdir(&parent_remote_id, &name).await {
            Ok(result) => {
                info!("Directory created: {} -> {}", path_str, result.id);
                Ok(())
            }
            Err(e) => {
                error!("Cozy mkdir failed: {}", e);
                Err(QueueError::CozyApi(e.to_string()))
            }
        }
    }

    pub async fn queue(&self, op: UploadOp) -> Result<(), QueueError> {
        self.sender.send(op).await.map_err(|_| QueueError::QueueClosed)
    }

    pub async fn shutdown(&self) -> Result<(), QueueError> {
        let mut handle_opt = self.worker_handle.write().await;
        if let Some(handle) = handle_opt.take() {
            handle.await.map_err(|_| QueueError::QueueClosed)?;
        }
        Ok(())
    }
}

impl Drop for UploadQueue {
    fn drop(&mut self) {
        let handle = self.worker_handle.clone();
        tokio::spawn(async move {
            let mut handle_opt = handle.write().await;
            if let Some(h) = handle_opt.take() {
                let _ = h.await;
            }
        });
    }
}
