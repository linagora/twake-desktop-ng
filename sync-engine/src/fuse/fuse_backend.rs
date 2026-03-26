use std::ffi::{OsStr, OsString};
use std::num::NonZeroU32;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use fuse3::raw::prelude::*;
use fuse3::Result as FuseResult;
use futures_util::stream::{self, Stream};
use tokio::sync::RwLock;

use crate::models::{FileNode, FileState};

const TTL: Duration = Duration::from_secs(1);
const ROOT_INODE: u64 = 1;

pub struct TwakeFuseFs {
    nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    children: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    next_inode: Arc<RwLock<u64>>,
}

impl TwakeFuseFs {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
        }
    }

    pub async fn register_node(&self, node: FileNode, parent_inode: u64) -> u64 {
        let mut next = self.next_inode.write().await;
        let inode = *next;
        *next += 1;

        self.path_to_inode.write().await.insert(node.path.clone(), inode);
        self.nodes.write().await.insert(inode, node);
        self.children.write().await.entry(parent_inode).or_default().push(inode);
        inode
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

    async fn open(&self, _req: Request, inode: u64, flags: u32) -> FuseResult<ReplyOpen> {
        let nodes = self.nodes.read().await;
        if !nodes.contains_key(&inode) && inode != ROOT_INODE {
            return Err(libc::ENOENT.into());
        }
        Ok(ReplyOpen { fh: 0, flags })
    }

    async fn read(
        &self, _req: Request, inode: u64, _fh: u64, _offset: u64, _size: u32,
    ) -> FuseResult<ReplyData> {
        let nodes = self.nodes.read().await;
        let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;

        if node.state == FileState::Ghost {
            return Err(libc::EIO.into());
        }

        Ok(ReplyData { data: Bytes::new() })
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
}
