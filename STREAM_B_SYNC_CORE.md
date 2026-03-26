# Stream B — Sync Core (Rust)

**Responsable:** Dev 2  
**Stack:** Rust, tokio, FUSE, SQLite  
**Objectif:** VFS avec placeholders + sync engine

---

## Jour 1 — Core Models et VFS Trait

### Matin (08:00 - 10:00) — Setup Cargo

**Tâche B1.1: Créer projet Rust**

```bash
cargo new sync-engine --lib
cd sync-engine
```

**Tâche B1.2: Cargo.toml**

```toml
[package]
name = "twake-sync"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1.35", features = ["full"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# UUID
uuid = { version = "1.6", features = ["v4", "serde"] }

# FUSE
fuse3 = { version = "0.8", features = ["tokio-runtime", "unprivileged"] }

# Byte buffers
bytes = "1"

# Futures
futures-util = "0.3"

# Libc
libc = "0.2"

# CLI
clap = { version = "4", features = ["derive"] }

# Database
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# IPC (will be shared with Stream C)
jsonrpsee = { version = "0.22", features = ["server"] }
```

**Tâche B1.3: Module structure**

```
sync-engine/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── models/
    │   ├── mod.rs
    │   ├── file_node.rs
    │   └── file_state.rs
    ├── vfs/
    │   ├── mod.rs
    │   └── vfs_trait.rs
    ├── db/
    │   ├── mod.rs
    │   └── repository.rs
    └── fuse/
        ├── mod.rs
        └── fuse_backend.rs
```

**Critère de succès:** `cargo build` passe

---

### Matin (10:00 - 12:00) — Models

**Tâche B1.4: FileState enum**

```rust
// src/models/file_state.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Ghost,        // Metadata only, not downloaded
    Hydrated,     // Content on disk, in sync
    Modified,     // Local changes pending sync
    Syncing,      // In progress
    Conflict,     // Conflict detected
    Error,        // Sync error
}

impl std::fmt::Display for FileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileState::Ghost => write!(f, "ghost"),
            FileState::Hydrated => write!(f, "hydrated"),
            FileState::Modified => write!(f, "modified"),
            FileState::Syncing => write!(f, "syncing"),
            FileState::Conflict => write!(f, "conflict"),
            FileState::Error => write!(f, "error"),
        }
    }
}
```

**Tâche B1.5: FileNode struct**

```rust
// src/models/file_node.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::path::PathBuf;

use super::file_state::FileState;

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
```

**Tâche B1.6: FileStatus (for IPC response)**

```rust
// src/models/mod.rs
pub mod file_node;
pub mod file_state;

pub use file_node::FileNode;
pub use file_state::FileState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,  // ISO 8601
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
```

**Critère de succès:** Models compilés et testables

---

### Après-midi (14:00 - 16:00) — VFS Trait

**Tâche B1.7: VFS trait definition**

```rust
// src/vfs/vfs_trait.rs
use crate::models::{FileNode, FileState};
use std::path::Path;
use async_trait::async_trait;

#[async_trait]
pub trait VfsBackend: Send + Sync {
    /// Mount the virtual file system
    async fn mount(&self, path: &Path) -> Result<(), VfsError>;

    /// Unmount the virtual file system
    async fn unmount(&self) -> Result<(), VfsError>;

    /// Get file/directory metadata
    async fn get_node(&self, path: &Path) -> Result<FileNode, VfsError>;

    /// List directory contents
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileNode>, VfsError>;

    /// Create a placeholder file (ghost)
    async fn create_placeholder(
        &self,
        path: &Path,
        metadata: FileMetadata,
    ) -> Result<(), VfsError>;

    /// Hydrate a ghost file (download content)
    async fn hydrate(&self, path: &Path) -> Result<(), VfsError>;

    /// Get file state
    async fn get_state(&self, path: &Path) -> Result<FileState, VfsError>;

    /// Update file state
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
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: String,
    pub is_dir: bool,
}
```

**Tâche B1.8: In-memory implementation (for testing)**

```rust
// src/vfs/mod.rs
pub mod vfs_trait;

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::RwLock;

use crate::models::{FileNode, FileState};
use super::vfs_trait::{VfsBackend, VfsError, FileMetadata};

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
        let prefix = path.to_str().unwrap();

        Ok(nodes.values()
            .filter(|n| n.path.starts_with(prefix))
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
```

**Critère de succès:** InMemoryVfs compilé et testé

---

### Après-midi (16:00 - 18:00) — FUSE Backend

**Tâche B1.9: FUSE setup**

```rust
// src/fuse/fuse_backend.rs
use std::ffi::{OsStr, OsString};
use std::num::NonZeroU32;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use fuse3::raw::prelude::*;
use fuse3::{MountOptions, Result as FuseResult};
use futures_util::stream;
use tokio::sync::RwLock;

use crate::models::{FileNode, FileState};

const TTL: Duration = Duration::from_secs(1);
const ROOT_INODE: u64 = 1;

/// Maps between FUSE inodes (u64) and our FileNode model.
/// fuse3 requires inode-based lookups, so we maintain a bidirectional mapping.
pub struct TwakeFuseFs {
    /// inode -> FileNode
    nodes: Arc<RwLock<HashMap<u64, FileNode>>>,
    /// path -> inode
    path_to_inode: Arc<RwLock<HashMap<String, u64>>>,
    /// parent_inode -> vec of child inodes
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

    /// Register a FileNode in the FUSE inode table. Called when syncing metadata.
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
        // TODO: trigger hydration here if state == Ghost
        Ok(ReplyOpen { fh: 0, flags })
    }

    async fn read(
        &self, _req: Request, inode: u64, _fh: u64, offset: u64, size: u32,
    ) -> FuseResult<ReplyData> {
        let nodes = self.nodes.read().await;
        let node = nodes.get(&inode).ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;

        if node.state == FileState::Ghost {
            // File not hydrated yet — return EIO to signal the app
            // In production, this triggers async hydration instead
            return Err(libc::EIO.into());
        }

        // TODO: read actual file content from cache directory
        // For MVP, return empty
        Ok(ReplyData { data: Bytes::new() })
    }

    async fn readdir(
        &self, _req: Request, inode: u64, _fh: u64, offset: i64,
    ) -> FuseResult<ReplyDirectory<impl Stream<Item = FuseResult<DirectoryEntry>> + Send + '_>> {
        let mut entries = vec![
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

        Ok(ReplyDirectory {
            entries: stream::iter(entries.into_iter().skip(offset as usize)),
        })
    }
}
```

**Tâche B1.10: Mount helper**

```rust
// src/fuse/mod.rs
pub mod fuse_backend;

use fuse3::raw::Session;
use fuse3::MountOptions;

pub async fn mount_fuse(
    fs: fuse_backend::TwakeFuseFs,
    mount_point: &str,
) -> Result<fuse3::raw::MountHandle, Box<dyn std::error::Error>> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

    let mut mount_options = MountOptions::default();
    mount_options.uid(uid).gid(gid).fs_name("twake");

    let mount_handle = Session::new(mount_options)
        .mount_with_unprivileged(fs, mount_point)
        .await?;

    Ok(mount_handle)
}
```

**Tâche B1.11: Simple FUSE with fuse3**

```rust
// src/bin/twake-vfs.rs
use std::path::PathBuf;
use clap::Parser;
use tracing::info;

use twake_sync::fuse::fuse_backend::TwakeFuseFs;
use twake_sync::fuse::mount_fuse;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "~/TwakeSync")]
    mount: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let fs = TwakeFuseFs::new();

    info!("Mounting FUSE at {:?}", args.mount);
    let mount_handle = mount_fuse(fs, args.mount.to_str().unwrap()).await?;

    info!("FUSE mounted. Press Ctrl+C to unmount.");
    mount_handle.await?;

    Ok(())
}
```

**Critère de succès:** FUSE mount point créé, placeholder visibles

---

## Jour 2 — Database et Hydration

### Matin (08:00 - 10:00) — SQLite Setup

**Tâche B2.1: Database schema**

```sql
-- sync-engine/migrations/001_initial.sql
CREATE TABLE IF NOT EXISTS file_nodes (
    id TEXT PRIMARY KEY,
    remote_id TEXT,
    path TEXT UNIQUE NOT NULL,
    state TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    modified TEXT NOT NULL,
    is_dir INTEGER NOT NULL DEFAULT 0,
    parent_id TEXT,
    FOREIGN KEY (parent_id) REFERENCES file_nodes(id)
);

CREATE INDEX IF NOT EXISTS idx_path ON file_nodes(path);
CREATE INDEX IF NOT EXISTS idx_state ON file_nodes(state);
```

**Tâche B2.2: Repository implementation**

```rust
// src/db/repository.rs
use sqlx::SqlitePool;
use crate::models::FileNode;
use crate::models::FileState;

pub struct FileRepository {
    pool: SqlitePool,
}

impl FileRepository {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn get(&self, path: &str) -> Result<Option<FileNode>, sqlx::Error> {
        let node = sqlx::query_as::<_, FileNode>(
            "SELECT * FROM file_nodes WHERE path = ?"
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;

        Ok(node)
    }

    pub async fn insert(&self, node: &FileNode) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO file_nodes (id, remote_id, path, state, size, modified, is_dir, parent_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(node.id.to_string())
        .bind(&node.remote_id)
        .bind(&node.path)
        .bind(node.state.to_string())
        .bind(node.size as i64)
        .bind(&node.modified)
        .bind(node.is_dir as i32)
        .bind(node.parent_id.map(|id| id.to_string()))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_state(&self, path: &str, state: FileState) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE file_nodes SET state = ? WHERE path = ?"
        )
        .bind(state.to_string())
        .bind(path)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_dir(&self, parent_path: &str) -> Result<Vec<FileNode>, sqlx::Error> {
        let nodes = sqlx::query_as::<_, FileNode>(
            "SELECT * FROM file_nodes WHERE parent_id = (SELECT id FROM file_nodes WHERE path = ?)"
        )
        .bind(parent_path)
        .fetch_all(&self.pool)
        .await?;

        Ok(nodes)
    }
}
```

**Critère de succès:** Database CRUD fonctionne

---

### Matin (10:00 - 12:00) — Hydration

**Tâche B2.3: Hydration service**

```rust
// src/services/hydration.rs
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::models::{FileNode, FileState};
use crate::vfs::vfs_trait::VfsBackend;
use crate::db::repository::FileRepository;

pub struct HydrationService {
    vfs: Box<dyn VfsBackend>,
    repo: FileRepository,
}

impl HydrationService {
    pub fn new(vfs: Box<dyn VfsBackend>, repo: FileRepository) -> Self {
        Self { vfs, repo }
    }

    pub async fn hydrate_file(&self, path: &Path) -> Result<(), HydrationError> {
        info!("Hydrating file: {}", path.display());

        // Get file metadata
        let node = self.vfs.get_node(path).await?;

        if node.state == FileState::Hydrated {
            info!("File already hydrated: {}", path.display());
            return Ok(());
        }

        // Update state to syncing
        self.vfs.set_state(path, FileState::Syncing).await?;

        // Download file content (placeholder for now)
        self.download_file(path, &node).await?;

        // Update state to hydrated
        self.vfs.set_state(path, FileState::Hydrated).await?;
        self.repo.update_state(path.to_str().unwrap(), FileState::Hydrated).await?;

        info!("File hydrated: {}", path.display());
        Ok(())
    }

    async fn download_file(&self, path: &Path, node: &FileNode) -> Result<(), HydrationError> {
        // For MVP: Create empty file
        // Future: Download from Twake API

        let parent = path.parent().ok_or(HydrationError::InvalidPath)?;
        tokio::fs::create_dir_all(parent).await?;

        let mut file = File::create(path).await?;
        file.write_all(b"").await?;  // Empty for MVP

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HydrationError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("VFS error: {0}")]
    Vfs(#[from] crate::vfs::vfs_trait::VfsError),

    #[error("Invalid path")]
    InvalidPath,
}
```

**Tâche B2.4: Integration with IPC**

```rust
// src/ipc/handlers.rs
use crate::models::FileStatus;
use crate::vfs::vfs_trait::VfsBackend;
use crate::services::hydration::HydrationService;

pub struct IpcHandlers {
    vfs: Box<dyn VfsBackend>,
    hydration: HydrationService,
}

impl IpcHandlers {
    pub async fn file_status(&self, path: &str) -> Result<FileStatus, IpcError> {
        let node = self.vfs.get_node(std::path::Path::new(path)).await?;
        Ok(FileStatus::from(&node))
    }

    pub async fn file_hydrate(&self, path: &str) -> Result<bool, IpcError> {
        self.hydration.hydrate_file(std::path::Path::new(path)).await?;
        Ok(true)
    }

    pub async fn file_list(&self, path: &str, recursive: bool) -> Result<Vec<crate::models::FileNode>, IpcError> {
        let nodes = self.vfs.list_dir(std::path::Path::new(path)).await?;
        Ok(nodes)
    }
}
```

**Critère de succès:** Hydration crée fichier sur disk

---

### Après-midi (14:00 - 18:00) — Integration et Demo

**Tâche B2.5: Main binary**

```rust
// src/bin/twake-vfs.rs
use std::path::PathBuf;
use std::sync::Arc;
use clap::Parser;
use tracing::info;

use twake_sync::db::repository::FileRepository;
use twake_sync::services::hydration::HydrationService;
use twake_sync::vfs::InMemoryVfs;
use twake_sync::vfs::vfs_trait::FileMetadata;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "~/TwakeSync")]
    mount: PathBuf,

    #[arg(short, long, default_value = "sqlite:twake.db")]
    database: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt::init();

    // Initialize components — use Arc for shared ownership
    let vfs = Arc::new(InMemoryVfs::new());
    let repo = FileRepository::new(&args.database).await?;
    let hydration = HydrationService::new(vfs.clone(), repo);

    // Create test data
    vfs.create_placeholder(
        PathBuf::from("~/TwakeSync/test.txt").as_path(),
        FileMetadata {
            size: 1024,
            modified: String::from("2026-01-01T00:00:00Z"),
            is_dir: false,
        },
    ).await?;

    info!("FUSE mounted at {:?}", args.mount);

    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
```

**Tâche B2.6: Test script**

```bash
#!/bin/bash
# test-hydration.sh

echo "1. Mounting FUSE..."
./target/release/twake-vfs --mount ~/TwakeSync &
sleep 2

echo "2. Listing files..."
ls -la ~/TwakeSync/

echo "3. Checking file status..."
# IPC call to get status

echo "4. Hydrating file..."
# IPC call to hydrate

echo "5. File content..."
cat ~/TwakeSync/test.txt
```

**Tâche B2.7: Demo data**

```rust
// src/bin/setup-demo.rs
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vfs = InMemoryVfs::new();

    // Create test structure
    vfs.create_placeholder(
        Path::new("~/TwakeSync/documents/test.txt"),
        FileMetadata { size: 1024, modified: String::from("2026-01-01T00:00:00Z"), is_dir: false },
    ).await?;

    vfs.create_placeholder(
        Path::new("~/TwakeSync/documents/photo.jpg"),
        FileMetadata { size: 102400, modified: String::from("2026-01-01T00:00:00Z"), is_dir: false },
    ).await?;

    vfs.create_placeholder(
        Path::new("~/TwakeSync/shared"),
        FileMetadata { size: 0, modified: String::from("2026-01-01T00:00:00Z"), is_dir: true },
    ).await?;

    Ok(())
}
```

**Critère de succès:** Demo data créé, hydrate fonctionne

---

## Build Commands

```bash
# Build
cargo build --release

# Run FUSE
./target/release/twake-vfs --mount ~/TwakeSync

# Run tests
cargo test
```

## Dependencies

```toml
# Required
- tokio (async runtime)
- fuse3 (FUSE bindings)
- sqlx (SQLite)
- serde (serialization)
- uuid (file IDs)
- clap (CLI)
- tracing (logging)
```

## Known Issues

- **FUSE permissions:** Uses fusermount3 (unprivileged), no sudo needed. Install: `apt install fuse3`
- **fuse3 crate:** Needs `libfuse3-dev` (`apt install libfuse3-dev`)
- **SQLite:** File-based, no concurrent writes
