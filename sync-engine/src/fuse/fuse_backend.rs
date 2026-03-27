use std::ffi::{OsStr, OsString};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use fuse3::raw::prelude::*;
use fuse3::Result as FuseResult;
use futures_util::stream;
use tokio::sync::RwLock;
use tokio::io::{AsyncWriteExt, AsyncSeekExt};
use tracing::{debug, info, warn};

use crate::cozy::CozyClient;
use crate::models::{FileNode, FileState};
use crate::services::upload_queue::{UploadOp, UploadQueue};

use uuid::Uuid;

use crate::cozy::client::{CozyEntry, ROOT_DIR_ID};
use std::collections::HashSet;

const TTL: Duration = Duration::from_secs(1);
pub const ROOT_INODE: u64 = 1;

/// Shared VFS state that can be used by an external sync loop.
#[derive(Clone)]
pub struct SharedVfsState {
    pub nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    pub path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    pub children: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    pub next_inode: Arc<RwLock<u64>>,
}

impl SharedVfsState {
    /// Sync local VFS state with remote Cozy entries.
    /// - Adds new remote files not present locally
    /// - Removes local ghost files that disappeared from remote
    pub async fn sync_with_remote(&self, remote_entries: &[CozyEntry]) {
        let remote_paths: HashSet<&str> = remote_entries.iter().map(|e| e.path.as_str()).collect();

        // 1. Remove local nodes that no longer exist on remote (only ghost/synced, not modified)
        let inodes_to_remove: Vec<(u64, String)> = {
            let nodes = self.nodes.read().await;
            nodes.iter()
                .filter(|(_, node)| {
                    node.remote_id.is_some()
                        && !remote_paths.contains(node.path.as_str())
                        && (node.state == FileState::Ghost || node.state == FileState::Synced || node.state == FileState::Hydrated)
                })
                .map(|(&ino, node)| (ino, node.path.clone()))
                .collect()
        };

        for (inode, path) in &inodes_to_remove {
            self.nodes.write().await.remove(inode);
            self.path_to_inode.write().await.remove(path);
            // Remove from parent's children list
            let mut children = self.children.write().await;
            for siblings in children.values_mut() {
                siblings.retain(|&i| i != *inode);
            }
            children.remove(inode);
            info!("Sync: removed local node {} (gone from remote)", path);
        }

        // 2. Update remote_id for local nodes that match by path but lack a remote_id
        {
            let path_to_inode = self.path_to_inode.read().await;
            let mut nodes = self.nodes.write().await;
            let remote_by_path: HashMap<&str, &CozyEntry> = remote_entries.iter()
                .map(|e| (e.path.as_str(), e))
                .collect();

            for (path, &inode) in path_to_inode.iter() {
                if let Some(node) = nodes.get_mut(&inode) {
                    if node.remote_id.is_none() {
                        if let Some(remote) = remote_by_path.get(path.as_str()) {
                            node.remote_id = Some(remote.id.clone());
                            if node.state == FileState::Syncing {
                                node.state = FileState::Synced;
                            }
                            info!("Sync: linked local {} to remote id {}", path, remote.id);
                        }
                    }
                }
            }
        }

        // 3. Add new remote entries not present locally
        let local_paths: HashSet<String> = self.path_to_inode.read().await.keys().cloned().collect();

        for entry in remote_entries {
            if local_paths.contains(&entry.path) {
                continue;
            }

            let parent_path = std::path::Path::new(&entry.path)
                .parent()
                .map(|p| p.to_str().unwrap_or("/"))
                .unwrap_or("/");
            let parent_path = if parent_path.is_empty() { "/" } else { parent_path };

            let parent_inode = self.path_to_inode.read().await
                .get(parent_path).copied()
                .unwrap_or(ROOT_INODE);

            let mut node = FileNode::new_ghost(&entry.path, entry.is_dir);
            node.remote_id = Some(entry.id.clone());
            node.size = entry.size;
            node.modified = entry.updated_at.clone();

            let mut next = self.next_inode.write().await;
            let inode = *next;
            *next += 1;

            self.path_to_inode.write().await.insert(entry.path.clone(), inode);
            self.nodes.write().await.insert(inode, node);
            self.children.write().await.entry(parent_inode).or_default().push(inode);

            info!("Sync: added new remote node {} (inode={})", entry.path, inode);
        }
    }
}

pub struct TwakeFuseFs {
    nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    children: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    next_inode: Arc<RwLock<u64>>,
    cache_dir: PathBuf,
    cozy: Option<CozyClient>,
    upload_queue: Option<Arc<UploadQueue>>,
}

impl TwakeFuseFs {
    pub fn new(cache_dir: PathBuf, cozy: CozyClient, upload_queue: Arc<UploadQueue>) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
            cache_dir,
            cozy: Some(cozy),
            upload_queue: Some(upload_queue),
        }
    }

    /// Create a TwakeFuseFs with Cozy but without upload queue (set later).
    pub fn new_without_upload_queue(cache_dir: PathBuf, cozy: CozyClient) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
            cache_dir,
            cozy: Some(cozy),
            upload_queue: None,
        }
    }

    /// Set the upload queue (breaks circular dependency).
    pub fn set_upload_queue(&mut self, queue: Arc<UploadQueue>) {
        self.upload_queue = Some(queue);
    }

    /// Create a TwakeFuseFs without a Cozy client (for tests).
    pub fn new_without_cozy(cache_dir: PathBuf) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
            cache_dir,
            cozy: None,
            upload_queue: None,
        }
    }

    /// Register a FileNode in the FUSE inode table. Returns the assigned inode.
    pub async fn register_node(&self, node: FileNode, parent_inode: u64) -> u64 {
        let mut next = self.next_inode.write().await;
        let inode = *next;
        *next += 1;

        self.path_to_inode.write().await.insert(node.path.clone(), inode);
        self.nodes.write().await.insert(inode, node);
        self.children.write().await.entry(parent_inode).or_default().push(inode);
        inode
    }

    /// Mark a node as hydrated and update its size.
    pub async fn mark_hydrated(&self, inode: u64, size: u64) {
        if let Some(node) = self.nodes.write().await.get_mut(&inode) {
            node.state = FileState::Hydrated;
            node.size = size;
        }
    }

    /// Get node inode by path.
    pub async fn get_node_by_path(&self, path: &str) -> Option<u64> {
        self.path_to_inode.read().await.get(path).copied()
    }

    /// Get mutable access to nodes map.
    pub async fn nodes_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, HashMap<u64, FileNode>> {
        self.nodes.write().await
    }

    /// Get shared handles for external sync loop.
    pub fn shared_state(&self) -> SharedVfsState {
        SharedVfsState {
            nodes: Arc::clone(&self.nodes),
            path_to_inode: Arc::clone(&self.path_to_inode),
            children: Arc::clone(&self.children),
            next_inode: Arc::clone(&self.next_inode),
        }
    }

    /// Get the cache file path for a given inode.
    fn cache_path(&self, node: &FileNode) -> PathBuf {
        self.cache_dir.join(node.id.to_string())
    }

    fn make_attr(inode: u64, node: &FileNode) -> FileAttr {
        let kind = if node.is_dir { FileType::Directory } else { FileType::RegularFile };
        let perm = if node.is_dir { 0o755 } else { 0o644 };
        let size = node.size; // Show actual size even for ghost files (better UX)

        FileAttr {
            ino: inode,
            size,
            blocks: 0,
            atime: SystemTime::now().into(),
            mtime: SystemTime::now().into(),
            ctime: SystemTime::now().into(),
            kind,
            perm,
            nlink: if node.is_dir { 2 } else { 1 },
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 0,
        }
    }
}

impl Filesystem for TwakeFuseFs {
    type DirEntryStream<'a> = futures_util::stream::Iter<std::vec::IntoIter<Result<DirectoryEntry, fuse3::Errno>>> where Self: 'a;
    type DirEntryPlusStream<'a> = futures_util::stream::Iter<std::vec::IntoIter<Result<DirectoryEntryPlus, fuse3::Errno>>> where Self: 'a;

    async fn init(&self, _req: Request) -> FuseResult<ReplyInit> {
        let nodes = self.nodes.read().await;
        let children = self.children.read().await;
        info!("FUSE init: {} nodes, children map: {:?}",
            nodes.len(),
            children.keys().collect::<Vec<_>>()
        );
        for (&ino, node) in nodes.iter() {
            info!("  inode={}: path={} is_dir={} state={}", ino, node.path, node.is_dir, node.state);
        }
        if let Some(root_children) = children.get(&ROOT_INODE) {
            info!("  root children: {:?}", root_children);
        }
        Ok(ReplyInit {
            max_write: NonZeroU32::new(16 * 1024).unwrap(),
        })
    }

    async fn destroy(&self, _req: Request) {}

    async fn lookup(&self, _req: Request, parent: u64, name: &OsStr) -> FuseResult<ReplyEntry> {
        debug!("LOOKUP parent={} name={:?}", parent, name);
        let children = self.children.read().await;
        let nodes = self.nodes.read().await;

        if let Some(child_inodes) = children.get(&parent) {
            for &inode in child_inodes {
                if let Some(node) = nodes.get(&inode) {
                    let node_name = std::path::Path::new(&node.path)
                        .file_name()
                        .unwrap_or_default();
                    if node_name == name {
                        debug!("LOOKUP found inode={} path={}", inode, node.path);
                        return Ok(ReplyEntry {
                            ttl: TTL,
                            attr: Self::make_attr(inode, node),
                            generation: 0,
                        });
                    }
                }
            }
        }
        debug!("LOOKUP ENOENT parent={} name={:?}", parent, name);
        Err(libc::ENOENT.into())
    }

    async fn getattr(
        &self, _req: Request, inode: u64, _fh: Option<u64>, _flags: u32,
    ) -> FuseResult<ReplyAttr> {
        if inode == ROOT_INODE {
            return Ok(ReplyAttr {
                ttl: TTL,
                attr: FileAttr {
                    ino: ROOT_INODE, size: 0, blocks: 0,
                    atime: SystemTime::now().into(),
                    mtime: SystemTime::now().into(),
                    ctime: SystemTime::now().into(),
                    kind: FileType::Directory, perm: 0o755,
                    nlink: 2,
                    uid: unsafe { libc::getuid() },
                    gid: unsafe { libc::getgid() },
                    rdev: 0, blksize: 0,
                },
            });
        }

        let nodes = self.nodes.read().await;
        let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;
        Ok(ReplyAttr { ttl: TTL, attr: Self::make_attr(inode, node) })
    }

    async fn setattr(
        &self,
        _req: Request,
        inode: u64,
        _fh: Option<u64>,
        _set_attr: SetAttr,
    ) -> FuseResult<ReplyAttr> {
        if inode == ROOT_INODE {
            return Ok(ReplyAttr {
                ttl: TTL,
                attr: FileAttr {
                    ino: ROOT_INODE, size: 0, blocks: 0,
                    atime: SystemTime::now().into(),
                    mtime: SystemTime::now().into(),
                    ctime: SystemTime::now().into(),
                    kind: FileType::Directory, perm: 0o755,
                    nlink: 2,
                    uid: unsafe { libc::getuid() },
                    gid: unsafe { libc::getgid() },
                    rdev: 0, blksize: 0,
                },
            });
        }

        let nodes = self.nodes.read().await;
        let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;
        Ok(ReplyAttr { ttl: TTL, attr: Self::make_attr(inode, node) })
    }

    async fn access(&self, _req: Request, _inode: u64, _mask: u32) -> FuseResult<()> {
        Ok(())
    }

    async fn statfs(&self, _req: Request, _inode: u64) -> FuseResult<ReplyStatFs> {
        Ok(ReplyStatFs {
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: 0,
            ffree: 0,
            bsize: 4096,
            namelen: 255,
            frsize: 4096,
        })
    }

    async fn opendir(&self, _req: Request, inode: u64, flags: u32) -> FuseResult<ReplyOpen> {
        if inode == ROOT_INODE {
            return Ok(ReplyOpen { fh: 0, flags });
        }
        let nodes = self.nodes.read().await;
        if nodes.get(&inode).map_or(true, |n| !n.is_dir) {
            return Err(libc::ENOENT.into());
        }
        Ok(ReplyOpen { fh: 0, flags })
    }

    async fn open(&self, _req: Request, inode: u64, flags: u32) -> FuseResult<ReplyOpen> {
        if inode == ROOT_INODE {
            return Ok(ReplyOpen { fh: 0, flags });
        }

        let needs_hydration = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;
            node.state == FileState::Ghost
        };

        if needs_hydration {
            let (cache_path, node_id, remote_id) = {
                let nodes = self.nodes.read().await;
                let node = nodes.get(&inode).unwrap();
                (self.cache_path(node), node.id, node.remote_id.clone())
            };

            info!("Hydrating ghost file (inode={}, id={})", inode, node_id);

            if let Err(e) = tokio::fs::create_dir_all(&self.cache_dir).await {
                warn!("Failed to create cache dir: {}", e);
                return Err(libc::EIO.into());
            }

            let content = match (&self.cozy, &remote_id) {
                (Some(cozy), Some(rid)) => {
                    info!("Downloading from Cozy: remote_id={}", rid);
                    match cozy.download(rid).await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(e) => {
                            warn!("Cozy download failed: {}", e);
                            return Err(libc::EIO.into());
                        }
                    }
                }
                _ => {
                    // Fallback placeholder when no Cozy client or no remote_id
                    format!("[twake] placeholder content for {}\n", node_id).into_bytes()
                }
            };

            if let Err(e) = tokio::fs::write(&cache_path, &content).await {
                warn!("Failed to write cache file: {}", e);
                return Err(libc::EIO.into());
            }

            let size = content.len() as u64;
            self.mark_hydrated(inode, size).await;
            info!("Hydrated inode={} ({} bytes)", inode, size);
        }

        Ok(ReplyOpen { fh: 0, flags })
    }

    async fn read(
        &self, _req: Request, inode: u64, _fh: u64, offset: u64, size: u32,
    ) -> FuseResult<ReplyData> {
        let nodes = self.nodes.read().await;
        let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;

        if node.state == FileState::Ghost {
            return Err(libc::EIO.into());
        }

        let cache_path = self.cache_path(node);
        let data = tokio::fs::read(&cache_path).await.map_err(|e| {
            warn!("Failed to read cache file {:?}: {}", cache_path, e);
            fuse3::Errno::from(libc::EIO)
        })?;

        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(ReplyData { data: Bytes::new() });
        }

        let end = std::cmp::min(offset + size as usize, data.len());
        Ok(ReplyData { data: Bytes::copy_from_slice(&data[offset..end]) })
    }

    async fn readdir(
        &self, _req: Request, inode: u64, _fh: u64, offset: i64,
    ) -> FuseResult<ReplyDirectory<Self::DirEntryStream<'_>>> {
        let mut entries: Vec<Result<DirectoryEntry, fuse3::Errno>> = vec![
            Ok(DirectoryEntry {
                inode, kind: FileType::Directory,
                name: OsString::from("."), offset: 1,
            }),
            Ok(DirectoryEntry {
                inode, kind: FileType::Directory,
                name: OsString::from(".."), offset: 2,
            }),
        ];

        let children = self.children.read().await;
        let nodes = self.nodes.read().await;

        if let Some(child_inodes) = children.get(&inode) {
            for (i, &child_ino) in child_inodes.iter().enumerate() {
                if let Some(node) = nodes.get(&child_ino) {
                    let kind = if node.is_dir { FileType::Directory } else { FileType::RegularFile };
                    let name = std::path::Path::new(&node.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_os_string();
                    entries.push(Ok(DirectoryEntry {
                        inode: child_ino, kind, name, offset: (i + 3) as i64,
                    }));
                }
            }
        }

        let skipped: Vec<_> = entries.into_iter().skip(offset as usize).collect();
        let entries = stream::iter(skipped);
        Ok(ReplyDirectory { entries })
    }

    async fn readdirplus(
        &self, _req: Request, inode: u64, _fh: u64, offset: u64, _lock_owner: u64,
    ) -> FuseResult<ReplyDirectoryPlus<Self::DirEntryPlusStream<'_>>> {
        let uid = unsafe { libc::getuid() };
        let gid = unsafe { libc::getgid() };

        let root_attr = FileAttr {
            ino: inode, size: 0, blocks: 0,
            atime: SystemTime::now().into(),
            mtime: SystemTime::now().into(),
            ctime: SystemTime::now().into(),
            kind: FileType::Directory, perm: 0o755,
            nlink: 2, uid, gid, rdev: 0, blksize: 0,
        };

        let mut entries: Vec<Result<DirectoryEntryPlus, fuse3::Errno>> = vec![
            Ok(DirectoryEntryPlus {
                inode, kind: FileType::Directory,
                name: OsString::from("."), offset: 1,
                attr: root_attr,
                entry_ttl: TTL, attr_ttl: TTL, generation: 0,
            }),
            Ok(DirectoryEntryPlus {
                inode, kind: FileType::Directory,
                name: OsString::from(".."), offset: 2,
                attr: root_attr,
                entry_ttl: TTL, attr_ttl: TTL, generation: 0,
            }),
        ];

        let children = self.children.read().await;
        let nodes = self.nodes.read().await;

        if let Some(child_inodes) = children.get(&inode) {
            for (i, &child_ino) in child_inodes.iter().enumerate() {
                if let Some(node) = nodes.get(&child_ino) {
                    let kind = if node.is_dir { FileType::Directory } else { FileType::RegularFile };
                    let name = std::path::Path::new(&node.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_os_string();
                    let attr = Self::make_attr(child_ino, node);
                    entries.push(Ok(DirectoryEntryPlus {
                        inode: child_ino, kind, name, offset: (i + 3) as i64,
                        attr,
                        entry_ttl: TTL, attr_ttl: TTL, generation: 0,
                    }));
                }
            }
        }

        let skipped: Vec<_> = entries.into_iter().skip(offset as usize).collect();
        let entries = stream::iter(skipped);
        Ok(ReplyDirectoryPlus { entries })
    }

    async fn create(
        &self,
        _req: Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        flags: u32,
    ) -> FuseResult<ReplyCreated> {
        let name_str = name.to_str().ok_or(libc::EINVAL)?;
        let (path, parent_uuid) = if parent == ROOT_INODE {
            (format!("/{}", name_str), None)
        } else {
            let nodes = self.nodes.read().await;
            let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
            let parent_path = &parent_node.path;
            let path = format!("{}/{}", parent_path.trim_end_matches('/'), name_str);
            (path, Some(parent_node.id))
        };

        let node = FileNode {
            id: Uuid::new_v4(),
            remote_id: None,
            path,
            state: FileState::Modified,
            size: 0,
            modified: chrono::Utc::now().to_rfc3339(),
            is_dir: false,
            parent_id: parent_uuid,
        };

        let inode = self.register_node(node, parent).await;
        let attr = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            Self::make_attr(inode, node)
        };

        info!("CREATE file: inode={}, parent={}, name={}", inode, parent, name.to_string_lossy());
        Ok(ReplyCreated {
            ttl: TTL,
            attr,
            generation: 0,
            fh: inode,
            flags,
        })
    }

    async fn write(
        &self,
        _req: Request,
        inode: u64,
        fh: u64,
        offset: u64,
        data: &[u8],
        write_flags: u32,
        flags: u32,
    ) -> FuseResult<ReplyWrite> {
        let cache_path = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            if node.is_dir {
                return Err(libc::EISDIR.into());
            }
            self.cache_path(node)
        };

        tokio::fs::create_dir_all(&self.cache_dir).await.map_err(|_| libc::EIO)?;

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&cache_path)
            .await
            .map_err(|_| libc::EIO)?;

        file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|_| libc::EIO)?;
        file.write_all(data).await.map_err(|_| libc::EIO)?;
        file.sync_all().await.map_err(|_| libc::EIO)?;

        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(&inode) {
            node.state = FileState::Modified;
            node.size = tokio::fs::metadata(&cache_path).await.map(|m| m.len()).unwrap_or(0);
            node.modified = chrono::Utc::now().to_rfc3339();
        }

        info!("Write: inode={}, offset={}, bytes={}", inode, offset, data.len());
        Ok(ReplyWrite { written: data.len() as u32 })
    }

    async fn flush(
        &self,
        _req: Request,
        inode: u64,
        fh: u64,
        lock_owner: u64,
    ) -> FuseResult<()> {
        if inode == ROOT_INODE {
            return Ok(());
        }

        let (content, path_str, remote_id, state, content_len) = {
            let mut nodes = self.nodes.write().await;
            let node = nodes.get_mut(&inode).ok_or(libc::ENOENT)?;

            // Skip if already queued for upload
            if node.state == FileState::Syncing || node.state == FileState::Synced {
                return Ok(());
            }

            let cache_path = self.cache_path(node);
            let path_str = node.path.clone();
            let remote_id = node.remote_id.clone();
            let state = node.state.clone();

            // Mark as syncing to prevent duplicate uploads
            node.state = FileState::Syncing;

            let content = match tokio::fs::read(&cache_path).await {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // No cache file = no content written yet (e.g. touch), upload empty
                    vec![]
                }
                Err(_) => return Err(libc::EIO.into()),
            };
            let content_len = content.len();
            (content, path_str, remote_id, state, content_len)
        };

        if let Some(queue) = &self.upload_queue {
            let op = match remote_id {
                Some(rid) => UploadOp::Update {
                    path: PathBuf::from(path_str),
                    content,
                    remote_id: rid,
                    rev: None,
                },
                None => {
                    let parent_remote_id = {
                        let nodes = self.nodes.read().await;
                        let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
                        
                        if let Some(parent_uuid) = node.parent_id {
                            let parent_inode = self.path_to_inode.read().await.iter()
                                .find(|(_, ino)| {
                                    nodes.get(*ino).map_or(false, |n| n.id == parent_uuid)
                                })
                                .map(|(_, ino)| *ino);
                            
                            if let Some(parent_ino) = parent_inode {
                                nodes.get(&parent_ino)
                                    .and_then(|p| p.remote_id.clone())
                                    .unwrap_or_else(|| "io.cozy.files.root-dir".to_string())
                            } else {
                                "io.cozy.files.root-dir".to_string()
                            }
                        } else {
                            "io.cozy.files.root-dir".to_string()
                        }
                    };
                    
                    UploadOp::Create {
                        path: PathBuf::from(path_str),
                        content,
                        parent_remote_id,
                    }
                }
            };

            if let Err(e) = queue.queue(op).await {
                warn!("Failed to queue upload: {:?}", e);
            }
        }

        info!("Flush queued upload: inode={}, bytes={}", inode, content_len);
        Ok(())
    }

    async fn fsync(
        &self,
        _req: Request,
        inode: u64,
        fh: u64,
        datasync: bool,
    ) -> FuseResult<()> {
        info!("Fsync: inode={}, datasync={}", inode, datasync);
        Ok(())
    }

    async fn mkdir(
        &self,
        _req: Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
    ) -> FuseResult<ReplyEntry> {
        let name_str = name.to_str().ok_or(libc::EINVAL)?;
        let (path, parent_uuid) = if parent == ROOT_INODE {
            (format!("/{}", name_str), None)
        } else {
            let nodes = self.nodes.read().await;
            let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
            let parent_path = &parent_node.path;
            let path = format!("{}/{}", parent_path.trim_end_matches('/'), name_str);
            (path, Some(parent_node.id))
        };

        let node = FileNode {
            id: Uuid::new_v4(),
            remote_id: None,
            path: path.clone(),
            state: FileState::Modified,
            size: 0,
            modified: chrono::Utc::now().to_rfc3339(),
            is_dir: true,
            parent_id: parent_uuid,
        };

        let inode = self.register_node(node, parent).await;
        let attr = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            Self::make_attr(inode, node)
        };

        if let Some(queue) = &self.upload_queue {
            let parent_remote_id = if parent == ROOT_INODE {
                "io.cozy.files.root-dir".to_string()
            } else {
                let nodes = self.nodes.read().await;
                let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
                parent_node.remote_id.clone().unwrap_or_else(|| "io.cozy.files.root-dir".to_string())
            };

            let op = UploadOp::Mkdir {
                path: PathBuf::from(&path),
                parent_remote_id,
                name: name.to_str().unwrap_or_default().to_string(),
            };
            if let Err(e) = queue.queue(op).await {
                warn!("Failed to queue mkdir: {:?}", e);
            }
        }

        info!("Created directory: inode={}, path={}", inode, path);
        Ok(ReplyEntry { ttl: TTL, attr, generation: 0 })
    }

    async fn rmdir(
        &self,
        _req: Request,
        parent: u64,
        name: &OsStr,
    ) -> FuseResult<()> {
        let (inode, children_empty) = {
            let nodes = self.nodes.read().await;
            let name_str = name.to_str().ok_or(libc::EINVAL)?;
            let parent_path = if parent == ROOT_INODE {
                "".to_string()
            } else {
                let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
                parent_node.path.trim_end_matches('/').to_string()
            };
            let path = format!("{}/{}", parent_path, name_str);
            
            let inode = self.path_to_inode.read().await.get(&path).copied().ok_or(libc::ENOENT)?;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            
            if !node.is_dir {
                return Err(libc::ENOTDIR.into());
            }

            let children_empty = self.children.read().await.get(&inode).map(|c| c.is_empty()).unwrap_or(true);
            (inode, children_empty)
        };

        if !children_empty {
            return Err(libc::ENOTEMPTY.into());
        }

        let remote_id_and_path = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            (node.remote_id.clone(), PathBuf::from(&node.path))
        };

        let mut nodes = self.nodes.write().await;
        nodes.remove(&inode);
        
        self.path_to_inode.write().await.retain(|_, v| *v != inode);
        self.children.write().await.remove(&inode);

        if let Some(siblings) = self.children.write().await.get_mut(&parent) {
            siblings.retain(|&i| i != inode);
        }

        if let Some(queue) = &self.upload_queue {
            if let (Some(rid), path_buf) = remote_id_and_path {
                let op = UploadOp::Delete {
                    path: path_buf,
                    remote_id: rid,
                    rev: None,
                };
                if let Err(e) = queue.queue(op).await {
                    warn!("Failed to queue rmdir delete: {:?}", e);
                }
            }
        }

        info!("Removed directory: inode={}", inode);
        Ok(())
    }

    async fn unlink(
        &self,
        _req: Request,
        parent: u64,
        name: &OsStr,
    ) -> FuseResult<()> {
        let (inode, cache_path) = {
            let nodes = self.nodes.read().await;
            let name_str = name.to_str().ok_or(libc::EINVAL)?;
            let parent_path = if parent == ROOT_INODE {
                "".to_string()
            } else {
                let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
                parent_node.path.trim_end_matches('/').to_string()
            };
            let path = format!("{}/{}", parent_path, name_str);
            
            let inode = self.path_to_inode.read().await.get(&path).copied().ok_or(libc::ENOENT)?;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            
            if node.is_dir {
                return Err(libc::EISDIR.into());
            }

            (inode, self.cache_path(node))
        };

        if cache_path.exists() {
            tokio::fs::remove_file(&cache_path).await.map_err(|_| libc::EIO)?;
        }

        let (remote_id, path_buf) = {
            let nodes = self.nodes.read().await;
            let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
            (node.remote_id.clone(), PathBuf::from(&node.path))
        };

        if let Some(queue) = &self.upload_queue {
            if let Some(rid) = remote_id {
                let op = UploadOp::Delete {
                    path: path_buf,
                    remote_id: rid,
                    rev: None,
                };
                if let Err(e) = queue.queue(op).await {
                    warn!("Failed to queue delete: {:?}", e);
                }
            }
        }

        let mut nodes = self.nodes.write().await;
        nodes.remove(&inode);
        
        self.path_to_inode.write().await.retain(|_, v| *v != inode);
        
        if let Some(siblings) = self.children.write().await.get_mut(&parent) {
            siblings.retain(|&i| i != inode);
        }

        info!("Unlinked file: inode={}", inode);
        Ok(())
    }

    async fn rename(
        &self,
        _req: Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
    ) -> FuseResult<()> {
        let (inode, old_path) = {
            let nodes = self.nodes.read().await;
            let name_str = name.to_str().ok_or(libc::EINVAL)?;
            let parent_path = if parent == ROOT_INODE {
                "".to_string()
            } else {
                let parent_node = nodes.get(&parent).ok_or(libc::ENOENT)?;
                parent_node.path.trim_end_matches('/').to_string()
            };
            let path = format!("{}/{}", parent_path, name_str);

            let inode = self.path_to_inode.read().await.get(&path).copied().ok_or(libc::ENOENT)?;
            (inode, path)
        };

        let new_path = {
            let new_name_str = newname.to_str().ok_or(libc::EINVAL)?;
            if newparent == ROOT_INODE {
                format!("/{}", new_name_str)
            } else {
                let nodes = self.nodes.read().await;
                let new_parent_node = nodes.get(&newparent).ok_or(libc::ENOENT)?;
                format!("{}/{}", new_parent_node.path.trim_end_matches('/'), new_name_str)
            }
        };

        {
            let mut nodes = self.nodes.write().await;
            if let Some(node) = nodes.get_mut(&inode) {
                node.path = new_path.clone();
            }
        }

        {
            let mut path_to_inode = self.path_to_inode.write().await;
            path_to_inode.remove(&old_path);
            path_to_inode.insert(new_path.clone(), inode);
        }

        {
            let mut children = self.children.write().await;
            if let Some(siblings) = children.get_mut(&parent) {
                siblings.retain(|&i| i != inode);
            }
            children.entry(newparent).or_default().push(inode);
        }

        if let Some(queue) = &self.upload_queue {
            let (remote_id, new_parent_remote_id, new_name_str) = {
                let nodes = self.nodes.read().await;
                let node = nodes.get(&inode).ok_or(libc::ENOENT)?;
                let new_parent_node = nodes.get(&newparent).ok_or(libc::ENOENT)?;
                (
                    node.remote_id.clone(),
                    new_parent_node.remote_id.clone(),
                    newname.to_str().map(|s| s.to_string()),
                )
            };

            if let (Some(rid), Some(new_parent_rid)) = (remote_id, new_parent_remote_id) {
                let op = UploadOp::Move {
                    path: PathBuf::from(&new_path),
                    remote_id: rid,
                    new_parent_id: new_parent_rid,
                    new_name: new_name_str,
                    rev: None,
                };
                if let Err(e) = queue.queue(op).await {
                    warn!("Failed to queue move: {:?}", e);
                }
            }
        }

        info!("Renamed: {} -> {}", old_path, new_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cozy::client::CozyEntry;
    use fuse3::raw::Filesystem;
    use futures_util::StreamExt;
    use tempfile::TempDir;

    fn req() -> Request {
        Request::default()
    }

    async fn setup() -> (TwakeFuseFs, TempDir) {
        let tmp = TempDir::new().unwrap();
        let fs = TwakeFuseFs::new_without_cozy(tmp.path().to_path_buf());

        let docs = FileNode::new_ghost("/documents", true);
        let docs_ino = fs.register_node(docs, ROOT_INODE).await;

        let mut file1 = FileNode::new_ghost("/documents/rapport.pdf", false);
        file1.size = 5_000_000;
        fs.register_node(file1, docs_ino).await;

        let mut file2 = FileNode::new_ghost("/documents/notes.txt", false);
        file2.size = 1024;
        fs.register_node(file2, docs_ino).await;

        (fs, tmp)
    }

    // -- register_node: inodes are sequential and parent-child links are built --

    #[tokio::test]
    async fn register_node_assigns_sequential_inodes() {
        let tmp = TempDir::new().unwrap();
        let fs = TwakeFuseFs::new_without_cozy(tmp.path().to_path_buf());

        let n1 = FileNode::new_ghost("/a", true);
        let n2 = FileNode::new_ghost("/b", true);
        let ino1 = fs.register_node(n1, ROOT_INODE).await;
        let ino2 = fs.register_node(n2, ROOT_INODE).await;

        assert_eq!(ino1, 2);
        assert_eq!(ino2, 3);
    }

    #[tokio::test]
    async fn register_node_builds_parent_child_map() {
        let (fs, _tmp) = setup().await;
        let children = fs.children.read().await;

        // root has 1 child (documents dir, inode 2)
        let root_kids = children.get(&ROOT_INODE).unwrap();
        assert_eq!(root_kids, &vec![2]);

        // documents dir (inode 2) has 2 children
        let doc_kids = children.get(&2).unwrap();
        assert_eq!(doc_kids.len(), 2);
    }

    // -- mark_hydrated: state and size actually change --

    #[tokio::test]
    async fn mark_hydrated_updates_state_and_size() {
        let (fs, _tmp) = setup().await;
        let file_ino = 3; // rapport.pdf

        {
            let nodes = fs.nodes.read().await;
            let node = nodes.get(&file_ino).unwrap();
            assert_eq!(node.state, FileState::Ghost);
        }

        fs.mark_hydrated(file_ino, 42).await;

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.state, FileState::Hydrated);
        assert_eq!(node.size, 42);
    }

    // -- lookup: finds child by name, returns ENOENT for missing --

    #[tokio::test]
    async fn lookup_finds_existing_child() {
        let (fs, _tmp) = setup().await;
        let reply = fs.lookup(req(), 2, OsStr::new("rapport.pdf")).await.unwrap();
        assert_eq!(reply.attr.ino, 3);
        assert_eq!(reply.attr.kind, FileType::RegularFile);
    }

    #[tokio::test]
    async fn lookup_returns_enoent_for_missing() {
        let (fs, _tmp) = setup().await;
        let err = fs.lookup(req(), 2, OsStr::new("nope.txt")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    #[tokio::test]
    async fn lookup_returns_enoent_for_wrong_parent() {
        let (fs, _tmp) = setup().await;
        // rapport.pdf is under inode 2, not under root
        let err = fs.lookup(req(), ROOT_INODE, OsStr::new("rapport.pdf")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    // -- getattr: correct type for root, dirs and files --

    #[tokio::test]
    async fn getattr_root_is_directory() {
        let (fs, _tmp) = setup().await;
        let reply = fs.getattr(req(), ROOT_INODE, None, 0).await.unwrap();
        assert_eq!(reply.attr.kind, FileType::Directory);
        assert_eq!(reply.attr.ino, ROOT_INODE);
    }

    #[tokio::test]
    async fn getattr_ghost_file_shows_actual_size() {
        let (fs, _tmp) = setup().await;
        let reply = fs.getattr(req(), 3, None, 0).await.unwrap();
        assert_eq!(reply.attr.kind, FileType::RegularFile);
        // ghost files report their actual metadata size (better UX)
        assert_eq!(reply.attr.size, 5_000_000); // rapport.pdf size from setup
    }

    #[tokio::test]
    async fn getattr_returns_enoent_for_bad_inode() {
        let (fs, _tmp) = setup().await;
        let err = fs.getattr(req(), 999, None, 0).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    // -- readdir: lists . , .. , and real children --

    #[tokio::test]
    async fn readdir_lists_children_with_dot_entries() {
        let (fs, _tmp) = setup().await;
        let reply = fs.readdir(req(), 2, 0, 0).await.unwrap();

        let entries: Vec<_> = reply.entries
            .collect::<Vec<_>>().await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let names: Vec<_> = entries.iter().map(|e| e.name.to_str().unwrap().to_string()).collect();
        assert!(names.contains(&".".to_string()));
        assert!(names.contains(&"..".to_string()));
        assert!(names.contains(&"rapport.pdf".to_string()));
        assert!(names.contains(&"notes.txt".to_string()));
        assert_eq!(entries.len(), 4); // . + .. + 2 files
    }

    #[tokio::test]
    async fn readdir_offset_skips_entries() {
        let (fs, _tmp) = setup().await;
        let reply = fs.readdir(req(), 2, 0, 2).await.unwrap();

        let entries: Vec<_> = reply.entries
            .collect::<Vec<_>>().await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // offset=2 skips . and .., leaves the 2 real files
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn readdir_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let fs = TwakeFuseFs::new_without_cozy(tmp.path().to_path_buf());
        let dir = FileNode::new_ghost("/empty", true);
        let dir_ino = fs.register_node(dir, ROOT_INODE).await;

        let reply = fs.readdir(req(), dir_ino, 0, 0).await.unwrap();
        let entries: Vec<_> = reply.entries.collect::<Vec<_>>().await;
        // only . and ..
        assert_eq!(entries.len(), 2);
    }

    // -- open + read: hydration creates a cache file and read returns its content --

    #[tokio::test]
    async fn open_hydrates_ghost_and_read_returns_content() {
        let (fs, _tmp) = setup().await;
        let file_ino = 4; // notes.txt

        // Before open: state is ghost
        {
            let nodes = fs.nodes.read().await;
            assert_eq!(nodes.get(&file_ino).unwrap().state, FileState::Ghost);
        }

        // open() triggers hydration
        fs.open(req(), file_ino, 0).await.unwrap();

        // After open: state is hydrated
        {
            let nodes = fs.nodes.read().await;
            assert_eq!(nodes.get(&file_ino).unwrap().state, FileState::Hydrated);
        }

        // read() returns actual content
        let reply = fs.read(req(), file_ino, 0, 0, 4096).await.unwrap();
        let content = String::from_utf8(reply.data.to_vec()).unwrap();
        assert!(content.starts_with("[twake] placeholder content for"));
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn open_already_hydrated_does_not_rehydrate() {
        let (fs, tmp) = setup().await;
        let file_ino = 3; // rapport.pdf

        // Hydrate once
        fs.open(req(), file_ino, 0).await.unwrap();
        let size_after_first = {
            let nodes = fs.nodes.read().await;
            nodes.get(&file_ino).unwrap().size
        };

        // Manually overwrite cache to detect if open re-hydrates
        let node_id = {
            let nodes = fs.nodes.read().await;
            nodes.get(&file_ino).unwrap().id
        };
        let cache_path = tmp.path().join(node_id.to_string());
        tokio::fs::write(&cache_path, b"custom data").await.unwrap();

        // Second open should not overwrite since state is already Hydrated
        fs.open(req(), file_ino, 0).await.unwrap();

        let data = tokio::fs::read(&cache_path).await.unwrap();
        assert_eq!(data, b"custom data");

        let nodes = fs.nodes.read().await;
        assert_eq!(nodes.get(&file_ino).unwrap().size, size_after_first);
    }

    #[tokio::test]
    async fn read_with_offset() {
        let (fs, _tmp) = setup().await;
        let file_ino = 4; // notes.txt

        fs.open(req(), file_ino, 0).await.unwrap();

        // Read full content first
        let full = fs.read(req(), file_ino, 0, 0, 4096).await.unwrap();
        let full_bytes = full.data.to_vec();
        assert!(full_bytes.len() > 10);

        // Read with offset=7, size=5
        let partial = fs.read(req(), file_ino, 0, 7, 5).await.unwrap();
        assert_eq!(partial.data.to_vec(), &full_bytes[7..12]);
    }

    #[tokio::test]
    async fn read_past_eof_returns_empty() {
        let (fs, _tmp) = setup().await;
        let file_ino = 4;
        fs.open(req(), file_ino, 0).await.unwrap();

        let reply = fs.read(req(), file_ino, 0, 99999, 4096).await.unwrap();
        assert!(reply.data.is_empty());
    }

    #[tokio::test]
    async fn read_ghost_returns_eio() {
        let (fs, _tmp) = setup().await;
        // Don't open — file stays ghost
        let err = fs.read(req(), 3, 0, 0, 4096).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::EIO));
    }

    // -- create: creates new file with Modified state --

    #[tokio::test]
    async fn create_new_file_assigns_inode_and_sets_modified_state() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2; // documents directory

        let reply = fs.create(req(), parent_ino, OsStr::new("newfile.txt"), 0o644, 0).await.unwrap();
        assert!(reply.fh > 0);

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&reply.fh).unwrap();
        assert_eq!(node.state, FileState::Modified);
        assert_eq!(node.path, "/documents/newfile.txt");
        assert!(!node.is_dir);
    }

    #[tokio::test]
    async fn create_file_in_nonexistent_parent_returns_enoent() {
        let (fs, _tmp) = setup().await;

        let err = fs.create(req(), 999, OsStr::new("file.txt"), 0o644, 0).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    // -- write: writes data to cache file and updates node --

    #[tokio::test]
    async fn write_updates_cache_and_node_state() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        // Create file first
        let create_reply = fs.create(req(), parent_ino, OsStr::new("writetest.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        // Write data
        let data = b"Hello, Twake!";
        let write_reply = fs.write(req(), file_ino, 0, 0, data, 0, 0).await.unwrap();
        assert_eq!(write_reply.written, data.len() as u32);

        // Verify node state
        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.state, FileState::Modified);
        assert_eq!(node.size, data.len() as u64);
    }

    #[tokio::test]
    async fn write_to_directory_returns_eisdir() {
        let (fs, _tmp) = setup().await;

        let err = fs.write(req(), 2, 0, 0, b"data", 0, 0).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::EISDIR));
    }

    #[tokio::test]
    async fn write_with_offset() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let create_reply = fs.create(req(), parent_ino, OsStr::new("offset.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        // Write at offset 0
        fs.write(req(), file_ino, 0, 0, b"Hello", 0, 0).await.unwrap();
        // Write at offset 5
        fs.write(req(), file_ino, 0, 5, b" World", 0, 0).await.unwrap();

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.size, 11); // "Hello World"
    }

    // -- flush: called after write operations --

    #[tokio::test]
    async fn flush_succeeds_for_modified_file() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let create_reply = fs.create(req(), parent_ino, OsStr::new("flushtest.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        fs.write(req(), file_ino, 0, 0, b"test data", 0, 0).await.unwrap();
        let flush_result = fs.flush(req(), file_ino, 0, 0).await;
        
        assert!(flush_result.is_ok());
    }

    // -- fsync: sync operation --

    #[tokio::test]
    async fn fsync_succeeds() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let create_reply = fs.create(req(), parent_ino, OsStr::new("synctest.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        let result = fs.fsync(req(), file_ino, 0, false).await;
        assert!(result.is_ok());
    }

    // -- mkdir: creates directory --

    #[tokio::test]
    async fn mkdir_creates_directory_with_modified_state() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2; // documents

        let reply = fs.mkdir(req(), parent_ino, OsStr::new("newdir"), 0o755, 0).await.unwrap();
        assert!(reply.ttl.as_secs() > 0);

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&reply.attr.ino).unwrap();
        assert!(node.is_dir);
        assert_eq!(node.state, FileState::Modified);
        assert_eq!(node.path, "/documents/newdir");
    }

    #[tokio::test]
    async fn mkdir_in_root_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let fs = TwakeFuseFs::new_without_cozy(tmp.path().to_path_buf());
        
        let root = FileNode::new_ghost("/", true);
        let root_ino = fs.register_node(root, ROOT_INODE).await;

        let reply = fs.mkdir(req(), root_ino, OsStr::new("rootdir"), 0o755, 0).await.unwrap();
        
        let nodes = fs.nodes.read().await;
        let node = nodes.get(&reply.attr.ino).unwrap();
        assert!(node.is_dir);
        assert_eq!(node.path, "/rootdir");
    }

    // -- rmdir: removes empty directory --

    #[tokio::test]
    async fn rmdir_removes_empty_directory() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        // Create directory
        let mkdir_reply = fs.mkdir(req(), parent_ino, OsStr::new("toberemoved"), 0o755, 0).await.unwrap();
        let dir_ino = mkdir_reply.attr.ino;

        // Remove it
        let rmdir_result = fs.rmdir(req(), parent_ino, OsStr::new("toberemoved")).await;
        assert!(rmdir_result.is_ok());

        // Verify it's gone
        let nodes = fs.nodes.read().await;
        assert!(!nodes.contains_key(&dir_ino));
    }

    #[tokio::test]
    async fn rmdir_nonempty_returns_enotempty() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        // Create directory
        let mkdir_reply = fs.mkdir(req(), parent_ino, OsStr::new("nonempty"), 0o755, 0).await.unwrap();
        let dir_ino = mkdir_reply.attr.ino;

        // Add a file to it
        fs.create(req(), dir_ino, OsStr::new("file.txt"), 0o644, 0).await.unwrap();

        // Try to remove - should fail
        let err = fs.rmdir(req(), parent_ino, OsStr::new("nonempty")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOTEMPTY));
    }

    #[tokio::test]
    async fn rmdir_on_file_returns_enotdir() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let err = fs.rmdir(req(), parent_ino, OsStr::new("rapport.pdf")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOTDIR));
    }

    // -- unlink: removes file --

    #[tokio::test]
    async fn unlink_removes_file() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        // Create file
        let create_reply = fs.create(req(), parent_ino, OsStr::new("todelete.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        // Unlink it
        let unlink_result = fs.unlink(req(), parent_ino, OsStr::new("todelete.txt")).await;
        assert!(unlink_result.is_ok());

        // Verify it's gone
        let nodes = fs.nodes.read().await;
        assert!(!nodes.contains_key(&file_ino));
    }

    #[tokio::test]
    async fn unlink_on_directory_returns_eisdir() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let mkdir_reply = fs.mkdir(req(), parent_ino, OsStr::new("subdir"), 0o755, 0).await.unwrap();
        
        let err = fs.unlink(req(), parent_ino, OsStr::new("subdir")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::EISDIR));
    }

    // -- rename: renames/moves file or directory --

    #[tokio::test]
    async fn rename_file_same_directory() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        // Create file
        let create_reply = fs.create(req(), parent_ino, OsStr::new("oldname.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        // Rename
        let rename_result = fs.rename(req(), parent_ino, OsStr::new("oldname.txt"), parent_ino, OsStr::new("newname.txt")).await;
        assert!(rename_result.is_ok());

        // Verify path updated
        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.path, "/documents/newname.txt");

        // Verify old path is gone from path_to_inode
        let path_map = fs.path_to_inode.read().await;
        assert!(!path_map.contains_key("/documents/oldname.txt"));
        assert!(path_map.contains_key("/documents/newname.txt"));
    }

    #[tokio::test]
    async fn rename_file_different_directory() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let mkdir_reply = fs.mkdir(req(), parent_ino, OsStr::new("newparent"), 0o755, 0).await.unwrap();
        let new_parent_ino = mkdir_reply.attr.ino;

        let create_reply = fs.create(req(), parent_ino, OsStr::new("moveme.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        let rename_result = fs.rename(req(), parent_ino, OsStr::new("moveme.txt"), new_parent_ino, OsStr::new("moveme.txt")).await;
        assert!(rename_result.is_ok());

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.path, "/documents/newparent/moveme.txt");
    }

    #[tokio::test]
    async fn rename_directory() {
        let (fs, _tmp) = setup().await;
        let parent_ino = 2;

        let mkdir_reply = fs.mkdir(req(), parent_ino, OsStr::new("olddir"), 0o755, 0).await.unwrap();
        let dir_ino = mkdir_reply.attr.ino;

        let rename_result = fs.rename(req(), parent_ino, OsStr::new("olddir"), parent_ino, OsStr::new("newdir")).await;
        assert!(rename_result.is_ok());

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&dir_ino).unwrap();
        assert_eq!(node.path, "/documents/newdir");
    }

    #[tokio::test]
    async fn rename_nonexistent_source_returns_enoent() {
        let (fs, _tmp) = setup().await;

        let err = fs.rename(req(), ROOT_INODE, OsStr::new("nonexistent"), ROOT_INODE, OsStr::new("newname")).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    // -- setattr: returns correct attrs --

    #[tokio::test]
    async fn setattr_root_returns_directory() {
        let (fs, _tmp) = setup().await;
        let reply = fs.setattr(req(), ROOT_INODE, None, SetAttr::default()).await.unwrap();
        assert_eq!(reply.attr.kind, FileType::Directory);
        assert_eq!(reply.attr.ino, ROOT_INODE);
    }

    #[tokio::test]
    async fn setattr_file_returns_attrs() {
        let (fs, _tmp) = setup().await;
        let reply = fs.setattr(req(), 3, None, SetAttr::default()).await.unwrap();
        assert_eq!(reply.attr.kind, FileType::RegularFile);
        assert_eq!(reply.attr.ino, 3);
        assert_eq!(reply.attr.size, 5_000_000);
    }

    #[tokio::test]
    async fn setattr_bad_inode_returns_enoent() {
        let (fs, _tmp) = setup().await;
        let err = fs.setattr(req(), 999, None, SetAttr::default()).await.unwrap_err();
        assert_eq!(err, fuse3::Errno::from(libc::ENOENT));
    }

    // -- access: always succeeds --

    #[tokio::test]
    async fn access_always_ok() {
        let (fs, _tmp) = setup().await;
        assert!(fs.access(req(), ROOT_INODE, 0).await.is_ok());
        assert!(fs.access(req(), 3, libc::R_OK as u32).await.is_ok());
        assert!(fs.access(req(), 999, 0).await.is_ok());
    }

    // -- create at ROOT_INODE: the main bug fix --

    #[tokio::test]
    async fn create_file_at_root() {
        let (fs, _tmp) = setup().await;

        let reply = fs.create(req(), ROOT_INODE, OsStr::new("rootfile.txt"), 0o644, 0).await.unwrap();
        let nodes = fs.nodes.read().await;
        let node = nodes.get(&reply.fh).unwrap();
        assert_eq!(node.path, "/rootfile.txt");
        assert_eq!(node.state, FileState::Modified);
        assert!(!node.is_dir);
        assert!(node.parent_id.is_none()); // root parent has no UUID
    }

    #[tokio::test]
    async fn create_file_at_root_appears_in_lookup() {
        let (fs, _tmp) = setup().await;

        fs.create(req(), ROOT_INODE, OsStr::new("findme.txt"), 0o644, 0).await.unwrap();

        let lookup = fs.lookup(req(), ROOT_INODE, OsStr::new("findme.txt")).await.unwrap();
        assert_eq!(lookup.attr.kind, FileType::RegularFile);
    }

    // -- mkdir at ROOT_INODE --

    #[tokio::test]
    async fn mkdir_at_root() {
        let (fs, _tmp) = setup().await;

        let reply = fs.mkdir(req(), ROOT_INODE, OsStr::new("rootdir"), 0o755, 0).await.unwrap();
        let nodes = fs.nodes.read().await;
        let node = nodes.get(&reply.attr.ino).unwrap();
        assert_eq!(node.path, "/rootdir");
        assert!(node.is_dir);
        assert_eq!(node.state, FileState::Modified);
    }

    // -- unlink at ROOT_INODE --

    #[tokio::test]
    async fn unlink_file_at_root() {
        let (fs, _tmp) = setup().await;

        let reply = fs.create(req(), ROOT_INODE, OsStr::new("bye.txt"), 0o644, 0).await.unwrap();
        let ino = reply.fh;

        fs.unlink(req(), ROOT_INODE, OsStr::new("bye.txt")).await.unwrap();

        let nodes = fs.nodes.read().await;
        assert!(!nodes.contains_key(&ino));
    }

    // -- rmdir at ROOT_INODE --

    #[tokio::test]
    async fn rmdir_at_root() {
        let (fs, _tmp) = setup().await;

        let reply = fs.mkdir(req(), ROOT_INODE, OsStr::new("tempdir"), 0o755, 0).await.unwrap();
        let ino = reply.attr.ino;

        fs.rmdir(req(), ROOT_INODE, OsStr::new("tempdir")).await.unwrap();

        let nodes = fs.nodes.read().await;
        assert!(!nodes.contains_key(&ino));
    }

    // -- rename at ROOT_INODE --

    #[tokio::test]
    async fn rename_file_from_root_to_subdir() {
        let (fs, _tmp) = setup().await;

        let create_reply = fs.create(req(), ROOT_INODE, OsStr::new("moveme.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        // Move from root to documents (inode 2)
        fs.rename(req(), ROOT_INODE, OsStr::new("moveme.txt"), 2, OsStr::new("moved.txt")).await.unwrap();

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.path, "/documents/moved.txt");
    }

    #[tokio::test]
    async fn rename_file_from_subdir_to_root() {
        let (fs, _tmp) = setup().await;

        let create_reply = fs.create(req(), 2, OsStr::new("insubdir.txt"), 0o644, 0).await.unwrap();
        let file_ino = create_reply.fh;

        fs.rename(req(), 2, OsStr::new("insubdir.txt"), ROOT_INODE, OsStr::new("atroot.txt")).await.unwrap();

        let nodes = fs.nodes.read().await;
        let node = nodes.get(&file_ino).unwrap();
        assert_eq!(node.path, "/atroot.txt");
    }

    // -- flush: edge cases --

    #[tokio::test]
    async fn flush_empty_file_without_cache_succeeds() {
        let (fs, _tmp) = setup().await;

        // Create file at root (no write, so no cache file)
        let reply = fs.create(req(), ROOT_INODE, OsStr::new("empty.txt"), 0o644, 0).await.unwrap();
        let ino = reply.fh;

        // Flush should succeed (queues upload of empty content)
        let result = fs.flush(req(), ino, 0, 0).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn flush_skips_syncing_state() {
        let (fs, _tmp) = setup().await;

        let reply = fs.create(req(), ROOT_INODE, OsStr::new("syncing.txt"), 0o644, 0).await.unwrap();
        let ino = reply.fh;

        // First flush changes state to Syncing
        fs.flush(req(), ino, 0, 0).await.unwrap();

        {
            let nodes = fs.nodes.read().await;
            assert_eq!(nodes.get(&ino).unwrap().state, FileState::Syncing);
        }

        // Second flush should be a no-op (state is Syncing)
        fs.flush(req(), ino, 0, 0).await.unwrap();
    }

    #[tokio::test]
    async fn flush_root_inode_is_noop() {
        let (fs, _tmp) = setup().await;
        assert!(fs.flush(req(), ROOT_INODE, 0, 0).await.is_ok());
    }

    // -- SharedVfsState::sync_with_remote --

    #[tokio::test]
    async fn sync_removes_deleted_remote_files() {
        let (fs, _tmp) = setup().await;
        let shared = fs.shared_state();

        // Add a node with remote_id
        let mut node = FileNode::new_ghost("/remote-file.txt", false);
        node.remote_id = Some("remote-123".to_string());
        let ino = fs.register_node(node, ROOT_INODE).await;

        // Sync with empty remote list — the file should be removed
        shared.sync_with_remote(&[]).await;

        let nodes = shared.nodes.read().await;
        assert!(!nodes.contains_key(&ino));
    }

    #[tokio::test]
    async fn sync_keeps_modified_local_files() {
        let (fs, _tmp) = setup().await;
        let shared = fs.shared_state();

        // Add a modified node with remote_id (locally changed)
        let mut node = FileNode::new_ghost("/edited.txt", false);
        node.remote_id = Some("remote-456".to_string());
        node.state = FileState::Modified;
        let ino = fs.register_node(node, ROOT_INODE).await;

        // Sync with empty remote — modified files should NOT be removed
        shared.sync_with_remote(&[]).await;

        let nodes = shared.nodes.read().await;
        assert!(nodes.contains_key(&ino));
    }

    #[tokio::test]
    async fn sync_adds_new_remote_files() {
        let (fs, _tmp) = setup().await;
        let shared = fs.shared_state();

        let remote_entries = vec![
            CozyEntry {
                id: "new-remote-id".to_string(),
                name: "newfile.txt".to_string(),
                path: "/newfile.txt".to_string(),
                is_dir: false,
                size: 42,
                updated_at: "2026-03-27T00:00:00Z".to_string(),
            },
        ];

        shared.sync_with_remote(&remote_entries).await;

        let path_map = shared.path_to_inode.read().await;
        assert!(path_map.contains_key("/newfile.txt"));

        let ino = *path_map.get("/newfile.txt").unwrap();
        let nodes = shared.nodes.read().await;
        let node = nodes.get(&ino).unwrap();
        assert_eq!(node.remote_id.as_deref(), Some("new-remote-id"));
        assert_eq!(node.size, 42);
    }

    #[tokio::test]
    async fn sync_links_remote_id_to_local_node() {
        let (fs, _tmp) = setup().await;
        let shared = fs.shared_state();

        // Create local node without remote_id (like after touch+upload)
        let mut node = FileNode::new_ghost("/uploaded.txt", false);
        node.state = FileState::Syncing;
        let ino = fs.register_node(node, ROOT_INODE).await;

        // Remote has same path with an id
        let remote_entries = vec![
            CozyEntry {
                id: "linked-remote-id".to_string(),
                name: "uploaded.txt".to_string(),
                path: "/uploaded.txt".to_string(),
                is_dir: false,
                size: 100,
                updated_at: "2026-03-27T00:00:00Z".to_string(),
            },
        ];

        shared.sync_with_remote(&remote_entries).await;

        let nodes = shared.nodes.read().await;
        let node = nodes.get(&ino).unwrap();
        assert_eq!(node.remote_id.as_deref(), Some("linked-remote-id"));
        assert_eq!(node.state, FileState::Synced);
    }

    #[tokio::test]
    async fn sync_does_not_duplicate_existing_entries() {
        let (fs, _tmp) = setup().await;
        let shared = fs.shared_state();

        let count_before = shared.nodes.read().await.len();

        // documents dir already exists at /documents (from setup)
        let remote_entries = vec![
            CozyEntry {
                id: "doc-id".to_string(),
                name: "documents".to_string(),
                path: "/documents".to_string(),
                is_dir: true,
                size: 0,
                updated_at: "2026-03-27T00:00:00Z".to_string(),
            },
        ];

        shared.sync_with_remote(&remote_entries).await;

        let count_after = shared.nodes.read().await.len();
        assert_eq!(count_before, count_after);
    }
}
