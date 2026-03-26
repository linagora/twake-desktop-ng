# VFS Engine Design Spec

**Date:** 2026-03-25  
**Component:** Sync Engine (Rust)  
**Stream:** B - Sync Core  
**Status:** Draft

---

## Overview

The VFS (Virtual File System) engine exposes synchronized files as a native file system with on-demand hydration. Users see placeholder files (ghosts) that are downloaded only when accessed.

**Key insight:** UUID is the true identity; path is just an alias. Essential for handling renames during editing.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    VFS Engine                           │
│                                                         │
│  ┌──────────────┐    ┌──────────────┐    ┌───────────┐ │
│  │ VFS Backend  │───▶│ File Node    │───▶│ Database  │ │
│  │ (FUSE/       │    │ Repository   │    │ (SQLite)  │ │
│  │  ProjFS)     │    │              │    │           │ │
│  └──────────────┘    └──────────────┘    └───────────┘ │
│         │                   │                    │      │
│         ▼                   ▼                    ▼      │
│  ┌───────────────────────────────────────────────────┐ │
│  │              File State Machine                   │ │
│  │  Ghost → Hydrated → Modified → Synced → Conflict │ │
│  └───────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

---

## Components

### 1. VFS Backend Trait

Common interface for platform-specific implementations:

```rust
#[async_trait]
pub trait VfsBackend: Send + Sync {
    async fn mount(&self, path: &Path) -> Result<(), VfsError>;
    async fn unmount(&self) -> Result<(), VfsError>;
    async fn get_node(&self, path: &Path) -> Result<FileNode, VfsError>;
    async fn list_dir(&self, path: &Path) -> Result<Vec<FileNode>, VfsError>;
    async fn create_placeholder(&self, path: &Path, metadata: FileMetadata) -> Result<(), VfsError>;
    async fn hydrate(&self, path: &Path) -> Result<(), VfsError>;
    async fn get_state(&self, path: &Path) -> Result<FileState, VfsError>;
    async fn set_state(&self, path: &Path, state: FileState) -> Result<(), VfsError>;
}
```

### 2. Platform Implementations

| Platform    | API          | Crate    | Status  |
| ----------- | ------------ | -------- | ------- |
| **Linux**   | FUSE         | `fuse3`  | ✅ MVP  |
| **Windows** | ProjFS       | `projfs` | Phase 3 |
| **macOS**   | FileProvider | FFI      | Phase 3 |

### 3. File Node Model

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,              // Immutable UUID (true identity)
    pub remote_id: Option<String>, // ID côté serveur Cozy
    pub path: String,          // Current path (alias, can change)
    pub state: FileState,
    pub size: u64,
    pub modified: String,      // ISO 8601 timestamp
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}

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
```

### 4. File State Machine

```
┌──────┐  open/read   ┌──────────┐  download   ┌──────────┐
│ Ghost│─────────────▶│ Syncing  │────────────▶│Hydrated  │
└──────┘              └──────────┘             └──────────┘
                          ▲                          │
                          │                          │ edit
                    error │                          ▼
                          │                 ┌──────────┐
                          └─────────────────│ Modified │
                                            └──────────┘
```

**Transitions:**

- `Ghost → Syncing`: File opened for read
- `Syncing → Hydrated`: Download complete
- `Syncing → Error`: Download failed
- `Hydrated → Modified`: File written
- `Modified → Syncing`: Sync triggered
- `Syncing → Hydrated`: Sync complete
- `Syncing → Conflict`: Conflict detected

---

## Data Flow

### Ghost File Creation

1. Remote sync receives file metadata
2. Create FileNode with state = Ghost
3. Store in SQLite database
4. Create placeholder file on VFS (size = 0 or 64 bytes with metadata)

### File Hydration

1. User opens ghost file (or calls `hydrate(path)`)
2. VFS intercepts `open()` call
3. IPC call to sync engine: `file.hydrate(path)`
4. Sync engine checks state = Ghost
5. Transition to Syncing
6. Download content from remote server
7. Write content to disk
8. Transition to Hydrated
9. Emit `FileChanged` event

### Local File Watch

1. File watcher (notify crate) detects local changes
2. Check file state
3. If Hydrated → Modified
4. Queue for sync to remote
5. Emit `FileChanged` event

---

## Database Schema

```sql
CREATE TABLE file_nodes (
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

CREATE INDEX idx_path ON file_nodes(path);
CREATE INDEX idx_state ON file_nodes(state);
```

---

## Error Handling

### VfsError Types

```rust
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

    #[error("Invalid state transition: {0} → {1}")]
    InvalidTransition(FileState, FileState),
}
```

### Recovery Strategies

- **FUSE crash** → Unmount and remount, preserve database
- **File locked** → Retry with exponential backoff
- **Disk full** → Emit error event, show UI notification
- **Network timeout** → Retry 3x, then mark as error

---

## Testing Strategy

### Unit Tests

- File state transitions
- Path resolution (UUID ↔ path)
- Database CRUD operations
- Serialization/deserialization

### Integration Tests

- FUSE mount/unmount
- Ghost file creation
- Hydration flow (mock network)
- Concurrent access

### E2E Tests

- Full sync cycle (remote → local → remote)
- Conflict detection and resolution
- Crash recovery

---

## Dependencies

```toml
[dependencies]
fuse3 = { version = "0.8", features = ["tokio-runtime", "unprivileged"] }
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
uuid = { version = "1.6", features = ["v4", "serde"] }
notify = "6.1"             # File watching
thiserror = "1.0"
bytes = "1"
futures-util = "0.3"
libc = "0.2"
```

---

## Risks and Mitigations

| Risk                     | Impact | Mitigation                                   |
| ------------------------ | ------ | -------------------------------------------- |
| **FUSE stability**       | High   | Isolate in separate process, auto-remount    |
| **Platform differences** | High   | Abstract behind trait, test on each platform |
| **File locking**         | Medium | Detect, emit error, show UI message          |
| **Disk space**           | Medium | Check before hydrate, warn user              |
| **Path encoding**        | Low    | Use UTF-8, reject invalid paths early        |

---

## References

- [STREAM_B_SYNC_CORE.md](../../STREAM_B_SYNC_CORE.md) - Implementation guide
- [INTERFACES.md](../../INTERFACES.md) - IPC contract
- [fuse3 crate](https://crates.io/crates/fuse3)
- [ProjFS documentation](https://docs.microsoft.com/en-us/windows/win32/fileio/projected-file-system)
