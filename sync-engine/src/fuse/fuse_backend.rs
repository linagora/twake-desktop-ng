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

use crate::cozy::CozyClient;
use crate::models::{FileNode, FileState};

const TTL: Duration = Duration::from_secs(1);
pub const ROOT_INODE: u64 = 1;

pub struct TwakeFuseFs {
    nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    children: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    next_inode: Arc<RwLock<u64>>,
    cache_dir: PathBuf,
    cozy: Option<CozyClient>,
}

impl TwakeFuseFs {
    pub fn new(cache_dir: PathBuf, cozy: CozyClient) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            path_to_inode: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
            next_inode: Arc::new(RwLock::new(ROOT_INODE + 1)),
            cache_dir,
            cozy: Some(cozy),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn getattr_ghost_file_has_zero_size() {
        let (fs, _tmp) = setup().await;
        let reply = fs.getattr(req(), 3, None, 0).await.unwrap();
        assert_eq!(reply.attr.kind, FileType::RegularFile);
        // ghost files report size 0
        assert_eq!(reply.attr.size, 0);
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
}
