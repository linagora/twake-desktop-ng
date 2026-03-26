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
use tracing::{info, warn};

use crate::models::{FileNode, FileState};

const TTL: Duration = Duration::from_secs(1);
pub const ROOT_INODE: u64 = 1;

pub struct TwakeFuseFs {
    nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    children: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    next_inode: Arc<RwLock<u64>>,
    cache_dir: PathBuf,
}

impl TwakeFuseFs {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
            cache_dir,
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

    /// Get the cache file path for a given inode.
    fn cache_path(&self, node: &FileNode) -> PathBuf {
        self.cache_dir.join(node.id.to_string())
    }

    fn make_attr(inode: u64, node: &FileNode) -> FileAttr {
        let kind = if node.is_dir { FileType::Directory } else { FileType::RegularFile };
        let perm = if node.is_dir { 0o755 } else { 0o644 };
        let size = if node.state == FileState::Ghost { 0 } else { node.size };

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
        let children = self.children.read().await;
        let nodes = self.nodes.read().await;

        if let Some(child_inodes) = children.get(&parent) {
            for &inode in child_inodes {
                if let Some(node) = nodes.get(&inode) {
                    let node_name = std::path::Path::new(&node.path)
                        .file_name()
                        .unwrap_or_default();
                    if node_name == name {
                        return Ok(ReplyEntry {
                            ttl: TTL,
                            attr: Self::make_attr(inode, node),
                            generation: 0,
                        });
                    }
                }
            }
        }
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
            // For MVP: create a dummy file in cache to simulate hydration
            // In production: this calls CozyClient.download() via a channel
            let (cache_path, node_id) = {
                let nodes = self.nodes.read().await;
                let node = nodes.get(&inode).unwrap();
                (self.cache_path(node), node.id)
            };

            info!("Hydrating ghost file (inode={}, id={})", inode, node_id);

            if let Err(e) = tokio::fs::create_dir_all(&self.cache_dir).await {
                warn!("Failed to create cache dir: {}", e);
                return Err(libc::EIO.into());
            }

            // MVP: write placeholder content so read() works
            let content = format!("[twake] placeholder content for {}\n", node_id);
            if let Err(e) = tokio::fs::write(&cache_path, content.as_bytes()).await {
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
}
