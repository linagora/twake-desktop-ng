# Plan: Read+Write Implementation for Twake Sync Engine

**Date:** 2026-03-27  
**Scope:** Full read+write support with async upload and conflict detection  
**Architecture:** Background sync queue with conflict resolution

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    User writes to FUSE                       │
│              ~/TwakeSync/docs/newfile.txt                    │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  FUSE Backend (fuse_backend.rs)                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ create()  → Create ghost + write to cache            │   │
│  │ write()   → Append to cache, mark Modified           │   │
│  │ flush()   → Queue for async upload                   │   │
│  │ mkdir()   → Create local + remote dir                │   │
│  │ unlink()  → Queue for async delete                   │   │
│  │ rename()  → Queue for async move                     │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  UploadQueue Service (services/upload_queue.rs)              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ - tokio::sync::mpsc channel                          │   │
│  │ - Background worker processes queue                  │   │
│  │ - Retry logic with exponential backoff               │   │
│  │ - State transitions: Modified → PendingSync → Synced │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  DeconflictionService (services/deconfliction.rs)            │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ - Compare local modified_at vs remote modified_at    │   │
│  │ - If conflict: create file.conflict.20260327.txt     │   │
│  │ - Last-write-wins for non-conflict cases             │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  CozyClient (cozy/client.rs)                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ upload()   → POST /files/:dir-id (create)            │   │
│  │ update()   → PUT /files/:file-id (update content)    │   │
│  │ delete()   → DELETE /files/:file-id                  │   │
│  │ mkdir()    → POST /folders (create directory)        │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## Work Units (Parallel Execution)

### Unit 1: FileState Extension

**File:** `src/models/file_state.rs`

**Changes:**
- Add `PendingSync` state (file modified locally, waiting for upload)
- Add `Conflict` state (conflict detected, user intervention needed)
- Update `From<&str>` parser
- Update `Display` trait

**New states:**
```rust
pub enum FileState {
    Ghost,       // Remote only, not downloaded
    Hydrated,    // Downloaded, in sync
    Modified,    // Written locally, pending upload
    PendingSync, // Upload in progress
    Synced,      // Successfully uploaded
    Conflict,    // Conflict detected
    Error,       // Error state
}
```

**Dependencies:** None  
**Estimated effort:** 15 min

---

### Unit 2: CozyClient Upload Methods

**File:** `src/cozy/client.rs`

**New methods:**

```rust
/// Upload a new file to Cozy
pub async fn upload(
    &self,
    dir_id: &str,
    name: &str,
    content: &[u8],
) -> Result<CozyFileResult, CozyError>

/// Update an existing file's content
pub async fn update(
    &self,
    file_id: &str,
    content: &[u8],
) -> Result<CozyFileResult, CozyError>

/// Delete a file from Cozy
pub async fn delete(&self, file_id: &str) -> Result<(), CozyError>

/// Create a directory
pub async fn mkdir(
    &self,
    parent_id: &str,
    name: &str,
) -> Result<CozyFolderResult, CozyError>

/// Move/rename a file
pub async fn r#move(
    &self,
    file_id: &str,
    new_dir_id: &str,
    new_name: Option<&str>,
) -> Result<CozyFileResult, CozyError>
```

**Response types:**
```rust
pub struct CozyFileResult {
    pub id: String,
    pub rev: String,
    pub path: String,
    pub updated_at: String,
}

pub struct CozyFolderResult {
    pub id: String,
    pub rev: String,
    pub path: String,
}
```

**Dependencies:** None  
**Estimated effort:** 45 min

---

### Unit 3: DeconflictionService

**File:** `src/services/deconfliction.rs` (new)

**Responsibilities:**
- Compare local metadata vs remote metadata
- Detect conflicts (both modified since last sync)
- Generate conflict filename: `file.conflict.20260327-143052.txt`
- Apply last-write-wins for non-conflict cases

**API:**
```rust
pub struct DeconflictionService {
    repo: FileRepository,
}

pub enum ConflictResult {
    NoConflict,
    Conflict {
        local_modified: String,
        remote_modified: String,
        conflict_path: PathBuf,
    },
}

impl DeconflictionService {
    pub fn new(repo: FileRepository) -> Self;
    
    pub async fn check_conflict(
        &self,
        path: &Path,
        remote_modified: &str,
    ) -> Result<ConflictResult, DeconflictError>;
    
    pub fn generate_conflict_path(&self, original: &Path) -> PathBuf;
}
```

**Dependencies:** FileRepository, FileNode  
**Estimated effort:** 30 min

---

### Unit 4: UploadQueue Service

**File:** `src/services/upload_queue.rs` (new)

**Responsibilities:**
- Background worker that processes upload queue
- Retry logic with exponential backoff
- State machine management
- Graceful shutdown

**Architecture:**
```rust
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
    },
    Delete {
        path: PathBuf,
        remote_id: String,
    },
    Move {
        path: PathBuf,
        remote_id: String,
        new_parent_id: String,
        new_name: Option<String>,
    },
}

pub struct UploadQueue {
    sender: mpsc::Sender<UploadOp>,
    worker_handle: JoinHandle<()>,
}

impl UploadQueue {
    pub fn new(
        cozy: CozyClient,
        repo: FileRepository,
        vfs: Arc<TwakeFuseFs>,
        deconfliction: DeconflictionService,
    ) -> Self;
    
    pub async fn queue(&self, op: UploadOp) -> Result<(), QueueError>;
    pub async fn shutdown(&self) -> Result<(), QueueError>;
}
```

**Worker loop:**
1. Pop operation from queue
2. Set state to `PendingSync`
3. Check for conflicts (for Update operations)
4. Call CozyClient method
5. On success: set state to `Synced`, update remote_id
6. On conflict: create conflict file, set state to `Conflict`
7. On error: retry with backoff (max 3 attempts), then `Error`

**Dependencies:** CozyClient, DeconflictionService, FileRepository, TwakeFuseFs  
**Estimated effort:** 60 min

---

### Unit 5: FUSE Write Operations

**File:** `src/fuse/fuse_backend.rs`

**New/Modified methods:**

```rust
/// create() — Create a new file
async fn create(
    &self,
    req: Request,
    parent: u64,
    name: &OsStr,
    mode: u32,
    flags: u32,
) -> FuseResult<ReplyCreate>

/// write() — Write data to file cache
async fn write(
    &self,
    req: Request,
    inode: u64,
    fh: u64,
    offset: u64,
    data: &[u8],
    flags: u32,
) -> FuseResult<ReplyWrite>

/// flush() — Trigger async upload
async fn flush(
    &self,
    req: Request,
    inode: u64,
    fh: u64,
    lock_owner: u64,
) -> FuseResult<ReplyEmpty>

/// fsync() — Ensure data is synced
async fn fsync(
    &self,
    req: Request,
    inode: u64,
    fh: u64,
    datasync: bool,
) -> FuseResult<ReplyEmpty>
```

**Implementation details:**

- **create():**
  - Create FileNode with state `Modified`
  - Assign inode, add to parent's children
  - Create empty cache file
  - Return ReplyCreate

- **write():**
  - Append data to cache file at offset
  - Keep state as `Modified` (already modified)
  - Update size metadata
  - Return bytes written

- **flush():**
  - Read full cache file content
  - Queue upload operation (create or update)
  - Return immediately (async upload)
  - Don't block on upload completion

**Dependencies:** UploadQueue (via Arc reference)  
**Estimated effort:** 60 min

---

### Unit 6: FUSE Directory Operations

**File:** `src/fuse/fuse_backend.rs`

**New methods:**

```rust
/// mkdir() — Create directory
async fn mkdir(
    &self,
    req: Request,
    parent: u64,
    name: &OsStr,
    mode: u32,
) -> FuseResult<ReplyEntry>

/// rmdir() — Remove directory
async fn rmdir(
    &self,
    req: Request,
    parent: u64,
    name: &OsStr,
) -> FuseResult<ReplyEmpty>

/// unlink() — Remove file
async fn unlink(
    &self,
    req: Request,
    parent: u64,
    name: &OsStr,
) -> FuseResult<ReplyEmpty>

/// rename() — Rename/move file or directory
async fn rename(
    &self,
    req: Request,
    parent: u64,
    name: &OsStr,
    newparent: u64,
    newname: &OsStr,
    flags: u32,
) -> FuseResult<ReplyEmpty>
```

**Implementation details:**

- **mkdir():**
  - Create local FileNode (directory)
  - Queue remote directory creation
  - Return ReplyEntry

- **rmdir():**
  - Check directory is empty
  - Queue remote deletion
  - Remove from local VFS

- **unlink():**
  - Delete cache file
  - Queue remote deletion
  - Remove from local VFS

- **rename():**
  - Update local path in FileNode
  - Update parent-child relationships
  - Queue remote move operation
  - Handle cross-directory moves

**Dependencies:** UploadQueue  
**Estimated effort:** 60 min

---

### Unit 7: Integration Tests

**File:** `src/fuse/fuse_backend.rs` (tests module)

**Test scenarios:**

1. **Write flow:**
   - create() → write() → flush()
   - Verify file in cache
   - Verify state transitions (Modified → PendingSync → Synced)

2. **Conflict detection:**
   - Modify file locally
   - Simulate remote modification with newer timestamp
   - flush() → verify conflict file created

3. **Directory operations:**
   - mkdir() → verify directory exists
   - Create file in directory
   - rmdir() → verify error (not empty)
   - unlink() file → rmdir() → verify success

4. **Rename/move:**
   - rename() within same directory
   - rename() across directories
   - Verify path updates correctly

5. **Upload queue:**
   - Queue multiple operations
   - Verify sequential processing
   - Verify retry on failure

**Dependencies:** All previous units  
**Estimated effort:** 45 min

---

## Execution Order

```
┌────────────────────────────────────────────────────────────┐
│ Phase 1: Foundation (Parallel)                              │
├────────────────────────────────────────────────────────────┤
│ Unit 1: FileState Extension                                 │
│ Unit 2: CozyClient Upload Methods                           │
└────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────┐
│ Phase 2: Services (Parallel)                                │
├────────────────────────────────────────────────────────────┤
│ Unit 3: DeconflictionService                                │
│ Unit 4: UploadQueue Service                                 │
└────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────┐
│ Phase 3: FUSE Implementation (Sequential)                   │
├────────────────────────────────────────────────────────────┤
│ Unit 5: FUSE Write Operations                               │
│ Unit 6: FUSE Directory Operations                           │
└────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌────────────────────────────────────────────────────────────┐
│ Phase 4: Testing                                            │
├────────────────────────────────────────────────────────────┤
│ Unit 7: Integration Tests                                   │
│ Full test suite + build verification                        │
└────────────────────────────────────────────────────────────┘
```

---

## Success Criteria

- [ ] All 32 existing tests still pass
- [ ] New integration tests pass (minimum 10 new tests)
- [ ] `cargo build` succeeds with no errors
- [ ] `cargo clippy` shows no new warnings
- [ ] Manual test: create file in FUSE mount → appears in Cozy
- [ ] Manual test: modify file → syncs to Cozy
- [ ] Manual test: conflict scenario → conflict file created

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Cozy API complexity | High | Start with minimal upload (create only), add update later |
| Async queue complexity | Medium | Use simple mpsc channel, avoid complex state |
| Conflict detection edge cases | Medium | Start with simple timestamp comparison |
| FUSE API learning curve | Medium | Reference existing read implementation |
| Performance (many small writes) | Low | Out of scope for MVP, document limitation |

---

## Notes

- **Async by design:** flush() returns immediately, upload happens in background
- **Conflict files:** Named `original.conflict.YYYYMMDD-HHMMSS.ext`
- **Retry policy:** 3 attempts with exponential backoff (1s, 2s, 4s)
- **Graceful shutdown:** UploadQueue must drain queue on shutdown
- **No write caching:** Each write() goes to cache file immediately
