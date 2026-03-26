# Reconciliation Engine Design Spec

**Date:** 2026-03-25  
**Component:** Sync Engine (Rust)  
**Stream:** B - Sync Core  
**Status:** Draft

---

## Overview

The Reconciliation Engine handles conflict detection and resolution when the same file is modified in multiple places. It uses a phased approach:

- **Phase 1:** CouchDB-style last-write-wins with manual backup
- **Phase 2:** Yjs/CRDT for automatic deterministic merge

**Design principle:** Abstract the backend from the start to enable migration without rewriting the entire system.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              Reconciliation Engine                       │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │              ReconciliationEngine                │   │
│  │              (trait / abstraction)               │   │
│  └─────────────────────────────────────────────────┘   │
│                     │              │                    │
│          ┌──────────┘              └──────────┐        │
│          ▼                                    ▼        │
│  ┌────────────────┐                  ┌────────────────┐│
│  │ CouchStyle     │                  │ YjsEngine      ││
│  │ Engine         │                  │ (Phase 2)      ││
│  │                │                  │                ││
│  │ - Last-write   │                  │ - CRDT merge   ││
│  │ - Backup copy  │                  │ - Auto resolve ││
│  │ - Manual       │                  │ - Real-time    ││
│  │   resolution   │                  │   collaboration││
│  └────────────────┘                  └────────────────┘│
└─────────────────────────────────────────────────────────┘
```

---

## Phase 1: CouchDB-Style (MVP)

### Strategy

**Last-write-wins with automatic backup:**

1. Compare versions (timestamp or vector clock)
2. Higher version wins
3. Lower version → create backup copy
4. User can manually resolve via UI

### Implementation

```rust
pub trait ReconciliationEngine: Send + Sync {
    async fn merge(
        &self,
        local: Document,
        remote: Document,
    ) -> Result<MergeResult, ReconciliationError>;

    async fn track_changes(&self, doc: Document) -> Result<ChangeSet, ReconciliationError>;

    async fn detect_conflicts(
        &self,
        local_rev: &Revision,
        remote_rev: &Revision,
    ) -> bool;
}

pub struct CouchStyleEngine {
    database: PouchDB,
    backup_dir: PathBuf,
}

pub enum MergeResult {
    LocalWins(Document),
    RemoteWins(Document),
    Conflict { local: Document, remote: Document, backup_path: PathBuf },
}
```

### Conflict Resolution Flow

```
┌──────────────┐
│  Sync starts │
└──────┬───────┘
       │
       ▼
┌──────────────────┐
│ Compare versions │
└──────┬───────────┘
       │
       ▼
┌──────────────────┐      ┌─────────────────┐
│  Local > Remote? │──Yes─▶│ Keep local,     │
└──────┬───────────┘      │ update remote   │
       │ No               └─────────────────┘
       ▼
┌──────────────────┐      ┌─────────────────┐
│ Remote > Local?  │──Yes─▶│ Keep remote,    │
└──────┬───────────┘      │ backup local    │
       │ No               └─────────────────┘
       ▼
┌──────────────────┐
│  CONFLICT        │
│ Backup both      │
│ Notify user      │
└──────────────────┘
```

### Backup Copy Naming

```
original.txt
original.txt.backup.2026-03-25T10-00-00.local
original.txt.backup.2026-03-25T10-00-00.remote
```

---

## Phase 2: Yjs/CRDT (Future)

### Strategy

**Deterministic merge using CRDT:**

1. Convert file content to Yjs document
2. Apply local and remote operations
3. CRDT guarantees consistent result
4. No conflicts, no data loss

### Implementation

```rust
pub struct YjsEngine {
    doc: YjsDocument,
    update_log: Vec<Update>,
}

impl ReconciliationEngine for YjsEngine {
    async fn merge(&self, local: Document, remote: Document) -> Result<MergeResult> {
        // CRDT merge: deterministic, no data loss
        let merged = self.doc.merge_ops(local.ops, remote.ops);
        Ok(MergeResult::Merged(merged))
    }
}
```

### Migration Path

```rust
pub enum ReconciliationBackend {
    Couch(CouchStyleEngine),
    Yjs(YjsEngine),
}

impl ReconciliationEngine for ReconciliationBackend {
    async fn merge(&self, local: Document, remote: Document) -> Result<MergeResult> {
        match self {
            ReconciliationBackend::Couch(engine) => engine.merge(local, remote).await,
            ReconciliationBackend::Yjs(engine) => engine.merge(local, remote).await,
        }
    }
}
```

---

## Data Models

### Document

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub path: String,
    pub content: Vec<u8>,
    pub version: Version,
    pub modified: OffsetDateTime,
    pub modified_by: Option<String>,
}
```

### Version

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Version {
    Timestamp(i64),           // Phase 1: Unix timestamp
    VectorClock(Vec<(String, i64)>),  // Phase 2: Per-user clocks
}

impl Version {
    pub fn compare(&self, other: &Version) -> Ordering {
        // Compare versions to determine which is newer
    }
}
```

### ChangeSet

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    pub file_id: Uuid,
    pub changes: Vec<Change>,
    pub timestamp: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Change {
    Insert { offset: usize, text: String },
    Delete { offset: usize, length: usize },
    Metadata { field: String, value: String },
}
```

---

## Conflict Detection

### When to Detect

1. **Before upload:** Local modified vs. remote version
2. **Before download:** Remote modified vs. local version
3. **Periodic scan:** Check all files for drift

### Detection Algorithm

```rust
async fn detect_conflicts(
    &self,
    local: &FileNode,
    remote: &FileMetadata,
) -> bool {
    // Both modified since last sync
    local.state == FileState::Modified
        && remote.modified > local.last_synced
}
```

---

## Error Handling

### ReconciliationError Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    #[error("Version parse error: {0}")]
    VersionParseError(String),

    #[error("Backup creation failed: {0}")]
    BackupError(String),

    #[error("Merge failed: {0}")]
    MergeError(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}
```

### Recovery Strategies

- **Backup fails** → Abort sync, emit error event
- **Merge fails** → Fall back to manual resolution
- **Database corrupted** → Restore from backup, re-sync

---

## Testing Strategy

### Unit Tests

- Version comparison logic
- Conflict detection algorithm
- Backup file naming
- State transitions

### Integration Tests

- Full merge flow (local vs. remote)
- Concurrent modifications
- Backup file creation
- Error recovery

### E2E Tests

- Two clients editing same file
- Offline editing → sync → conflict
- Manual resolution via UI

---

## Dependencies

```toml
[dependencies]
# Phase 1
pouchdb-rs = "0.1"            # CouchDB client (if needed)

# Phase 2 (not yet implemented)
y-crdt = "0.2"                # CRDT implementation

# Common
uuid = { version = "1.6", features = ["v4", "serde"] }
serde = { version = "1.0", features = ["derive"] }
time = { version = "0.3", features = ["serde"] }
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }
thiserror = "1.0"
```

---

## Risks and Mitigations

| Risk                 | Impact   | Mitigation                               |
| -------------------- | -------- | ---------------------------------------- |
| **Data loss**        | Critical | Always backup before overwrite           |
| **Silent conflicts** | High     | Detect and notify, never auto-override   |
| **Performance**      | Medium   | Batch operations, debounce sync          |
| **CRDT complexity**  | High     | Phase 1 first, abstract trait from start |
| **Version drift**    | Medium   | Vector clocks in Phase 2                 |

---

## Migration Checklist (Phase 1 → Phase 2)

- [ ] Define ReconciliationEngine trait
- [ ] Implement CouchStyleEngine
- [ ] Test conflict resolution manually
- [ ] Design Yjs data model
- [ ] Implement YjsEngine
- [ ] Test CRDT merge
- [ ] Add feature flag for backend selection
- [ ] Migration script (Couch → Yjs)
- [ ] Deprecate CouchStyleEngine

---

## References

- [STREAM_B_SYNC_CORE.md](../../STREAM_B_SYNC_CORE.md) - Implementation guide
- [CouchDB Replication](https://cloudant.com/blog/couchdb-replication-explained)
- [Yjs Documentation](https://yjs.dev/)
- [CRDT Introduction](https://crdt.tech/)
