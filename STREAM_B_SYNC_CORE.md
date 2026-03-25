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
fuse3 = "0.3"

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
    Error,        // Sync error
}

impl std::fmt::Display for FileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileState::Ghost => write!(f, "ghost"),
            FileState::Hydrated => write!(f, "hydrated"),
            FileState::Modified => write!(f, "modified"),
            FileState::Syncing => write!(f, "syncing"),
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
use time::OffsetDateTime;

use super::file_state::FileState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: OffsetDateTime,
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}

impl FileNode {
    pub fn new_ghost(path: &str, is_dir: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            path: path.to_string(),
            state: FileState::Ghost,
            size: 0,
            modified: OffsetDateTime::now_utc(),
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
            modified: node.modified.to_rfc3339(),
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
    pub modified: time::OffsetDateTime,
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
use std::path::Path;
use std::os::unix::ffi::OsStrExt;
use fuse3::{FileSystem, Reply, Request};
use fuse3::ffi::FileType;

use crate::models::{FileNode, FileState};
use crate::vfs::vfs_trait::{VfsBackend, VfsError, FileMetadata};

pub struct FuseBackend {
    vfs: Box<dyn VfsBackend>,
    mount_point: String,
}

impl FuseBackend {
    pub fn new(vfs: Box<dyn VfsBackend>) -> Self {
        Self {
            vfs,
            mount_point: String::new(),
        }
    }
}

#[async_trait]
impl FileSystem for FuseBackend {
    type Error = VfsError;
    
    async fn lookup(&self, req: &Request<'_>, parent: u64, name: &std::ffi::OsStr) -> Result<Reply::Lookup, Self::Error> {
        let path = self.path_from_parent(parent, name);
        let node = self.vfs.get_node(Path::new(&path)).await?;
        
        Ok(Reply::Lookup {
            entry: fuse3::Entry {
                inode: node.id.as_u128() as u64,
                generation: 1,
                attr: self.node_to_attr(&node),
                attr_timeout: 3600,
                entry_timeout: 3600,
            },
        })
    }
    
    async fn getattr(&self, req: &Request<'_>, ino: u64) -> Result<Reply::Attr, Self::Error> {
        // Get inode from ID mapping
        Ok(Reply::Attr {
            attr: fuse3::Attr {
                ino,
                size: 0,
                blocks: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
                atim: 0,
                mtim: 0,
                ctim: 0,
                kind: FileType::RegularFile,
                perm: 0o644,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
            },
            timeout: 3600,
        })
    }
    
    async fn readdir(&self, req: &Request<'_>, ino: u64, offset: i64) -> Result<Reply::Entry, Self::Error> {
        // List directory entries
        Ok(Reply::Entry { entries: vec![] })
    }
    
    async fn open(&self, req: &Request<'_>, ino: u64, flags: i32) -> Result<Reply::Open, Self::Error> {
        // Trigger hydration if ghost
        Ok(Reply::Open { fh: 0, flags: flags as u32 })
    }
    
    async fn read(&self, req: &Request<'_>, ino: u64, fh: u64, offset: i64, size: u32) -> Result<Reply::Data, Self::Error> {
        // Read file content
        Ok(Reply::Data { data: vec![] })
    }
}
```

**Tâche B1.10: Mount helper**
```rust
// src/fuse/mod.rs
pub mod fuse_backend;

use std::path::Path;
use tokio::process::Command;

pub async fn mount_fuse(mount_point: &Path) -> Result<(), std::io::Error> {
    // Create mount point
    tokio::fs::create_dir_all(mount_point).await?;
    
    // Mount FUSE filesystem
    // For MVP, we'll use a simpler approach with fuse3
    
    Ok(())
}
```

**Tâche B1.11: Simple FUSE with fuse3**
```rust
// src/bin/twake-vfs.rs
use std::path::PathBuf;
use clap::Parser;
use tracing::info;

use twake_sync::fuse::fuse_backend::FuseBackend;
use twake_sync::vfs::vfs_trait::VfsBackend;
use twake_sync::models::InMemoryVfs;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    mount: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    tracing_subscriber::fmt::init();
    
    let vfs = InMemoryVfs::new();
    let fuse = FuseBackend::new(Box::new(vfs));
    
    info!("Mounting FUSE at {:?}", args.mount);
    fuse.mount(&args.mount).await?;
    
    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
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
            "INSERT OR REPLACE INTO file_nodes (id, path, state, size, modified, is_dir, parent_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(node.id.to_string())
        .bind(&node.path)
        .bind(node.state.to_string())
        .bind(node.size as i64)
        .bind(node.modified.to_rfc3339())
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
    
    pub async fn list_dir(&self, path: &str) -> Result<Vec<FileNode>, sqlx::Error> {
        let nodes = sqlx::query_as::<_, FileNode>(
            "SELECT * FROM file_nodes WHERE path LIKE ? || '%' AND path != ?"
        )
        .bind(path)
        .bind(path)
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
use clap::Parser;
use tracing::info;

use twake_sync::db::repository::FileRepository;
use twake_sync::services::hydration::HydrationService;
use twake_sync::models::InMemoryVfs;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "/mnt/twake")]
    mount: PathBuf,
    
    #[arg(short, long, default_value = "sqlite:twake.db")]
    database: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    tracing_subscriber::fmt::init();
    
    // Initialize components
    let vfs = Box::new(InMemoryVfs::new());
    let repo = FileRepository::new(&args.database).await?;
    let hydration = HydrationService::new(vfs.clone(), repo);
    
    // Create test data
    hydration.vfs.create_placeholder(
        PathBuf::from("/mnt/twake/test.txt").as_path(),
        FileMetadata {
            size: 1024,
            modified: time::OffsetDateTime::now_utc(),
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
sudo ./target/release/twake-vfs --mount /mnt/twake &
sleep 2

echo "2. Listing files..."
ls -la /mnt/twake/

echo "3. Checking file status..."
# IPC call to get status

echo "4. Hydrating file..."
# IPC call to hydrate

echo "5. File content..."
cat /mnt/twake/test.txt
```

**Tâche B2.7: Demo data**
```rust
// src/bin/setup-demo.rs
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vfs = InMemoryVfs::new();
    
    // Create test structure
    vfs.create_placeholder(
        Path::new("/mnt/twake/documents/test.txt"),
        FileMetadata { size: 1024, modified: now(), is_dir: false },
    ).await?;
    
    vfs.create_placeholder(
        Path::new("/mnt/twake/documents/photo.jpg"),
        FileMetadata { size: 102400, modified: now(), is_dir: false },
    ).await?;
    
    vfs.create_placeholder(
        Path::new("/mnt/twake/shared"),
        FileMetadata { size: 0, modified: now(), is_dir: true },
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
sudo ./target/release/twake-vfs --mount /mnt/twake

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

- **FUSE permissions:** Need sudo for mount
- **fuse3 crate:** May need system FUSE dev headers
- **SQLite:** File-based, no concurrent writes
