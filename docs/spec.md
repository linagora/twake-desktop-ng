# Twake Desktop NG - Technical Specification

**Version:** 4.0  
**Date:** 2026-03-25  
**Status:** Final

---

## Executive Summary

Twake Desktop NG is a **collaborative work platform** (not just a file sync client) that provides an enriched desktop experience for Twake web services. The architecture combines a **native C++ shell with CEF** for web rendering with a **Rust sync engine** for VFS, reconciliation, and network operations.

**Key differentiator:** Multi-actor collaboration with offline support, where users can edit the same document via different interfaces (local VFS editor vs. OnlyOffice web) with automatic reconciliation.

**Architecture:** Two-process model with CEF shell (C++) and sync engine (Rust) communicating via IPC.

---

## Project Objectives

### Primary Goals

1. **Virtual File System (VFS)** — Expose synchronized file system with placeholder files (on-demand hydration)
2. **Web Services on Desktop** — Open Twake web apps (app1.twake.app, app2.twake.app, etc.) in native windows
3. **Offline-first** — Full offline support with deferred sync and conflict reconciliation
4. **Unified Notifications** — Centralized notifications for all Twake services
5. **Embedded Editing** — Integrate editors (OnlyOffice) for VFS files with collaborative mode

### MVP Scope

1. ✅ Authenticate via OIDC PKCE
2. ✅ Synchronize a local directory with remote (VFS with placeholders)
3. ✅ Open a web app in a local window (online)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    CEF Shell (C++)                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │   Drive     │  │    Mail     │  │  Calendar   │  ...    │
│  │  WebView    │  │  WebView    │  │  WebView    │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│                                                             │
│  - Window management (native OS)                            │
│  - Tray icon + OS notifications                             │
│  - Bridge JS ↔ IPC (window.__twake)                         │
│  - Security: inject bridge only on Twake domains            │
└───────────────────┬─────────────────────────────────────────┘
                    │ Unix socket / JSON-RPC
                    ▼
┌─────────────────────────────────────────────────────────────┐
│                  Sync Engine (Rust)                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    VFS       │  │ Reconcile    │  │   Network    │       │
│  │              │  │   Engine     │  │   Layer      │       │
│  │  - FUSE      │  │              │  │              │       │
│  │  - ProjFS    │  │  - Phase 1:  │  │  - WebSocket │       │
│  │  - FileProv  │  │    CouchDB   │  │  - SSE       │       │
│  │              │  │  - Phase 2:  │  │  - HTTP      │       │
│  │  State:      │  │    Yjs/CRDT  │  │              │       │
│  │  Ghost       │  │              │  │              │       │
│  │  Hydrated    │  │ Strategy:    │  │              │       │
│  │  Modified    │  │  - Last-write│  │              │       │
│  │  Synced      │  │    wins +    │  │              │       │
│  │  Conflict    │  │    backup    │  │              │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                             │
│  - File state tracking (Ghost/Hydrated/Modified/Synced)     │
│  - Conflict detection and resolution                        │
│  - Event bus (tokio::broadcast)                             │
│  - Event aggregation for inter-WebView communication        │
└─────────────────────────────────────────────────────────────┘
```

### Process Model

**Two separate processes:**

1. **CEF Shell (C++)** — ~100MB RAM, handles UI, WebViews, native integration
2. **Sync Engine (Rust)** — ~30MB RAM, handles VFS, sync, reconciliation, network

**Benefits:**
- Isolation: WebView crash ≠ sync engine crash
- Security: C++ shell minimal, Rust engine sandboxed
- Performance: Separate CPU cores, no contention
- Maintainability: Clear separation of concerns

---

## Shell Layer: CEF (Chromium Embedded Framework)

### Why CEF

| Criteria | CEF | Electron | Tauri |
|----------|-----|----------|-------|
| **Rendu web apps** | ✅ Chromium | ✅ Chromium | ⚠️ WebView système |
| **Poids install** | ~100MB | ~150MB | ~10MB |
| **RAM idle** | ~100-120MB | ~200-300MB | ~30MB |
| **Node.js exposé** | ✅ Non | ⚠️ Oui | ✅ Non |
| **Langage shell** | C++ | JS/TS | Rust |
| **Intégration Rust** | ⚠️ IPC | ⚠️ IPC | ✅ Direct |
| **Packaging** | ⚠️ Complexe | ✅ Trivial | ✅ Simple |
| **Maturité** | ✅ 17 ans | ✅ Très mature | 🟡 Jeune |
| **Isolation crash** | ✅ Renderer process | ⚠️ Node.js | ✅ WebView |

**Decision: CEF**

**Rationale:**
1. **Rendu garanti identique** — Chromium bundlé, même comportement que Chrome
2. **Sécurité** — Pas de Node.js exposé, surface d'attaque réduite
3. **Stabilité** — 17 ans d'existence, utilisé par Adobe, Spotify, Riot Games
4. **Isolation des renderer processes** — Une WebView qui crash n'emporte pas les autres

**Trade-offs:**
- C++ pour le shell (500-1000 lignes, maintenable)
- Packaging plus complexe (build system CEF)
- Compétences C++ nécessaires (déjà dans l'équipe)

### CEF Configuration

**Renderer process isolation:**

```cpp
// CEF Settings
CefSettings settings;
settings.multi_threaded_message_loop = true;
settings.external_message_pump = false;

// Force one renderer process per origin (default)
// Each app1.twake.app, app2.twake.app gets separate process
// If isolation between same-origin windows needed:
command_line->AppendSwitch("process-per-tab");
```

**Behavior:**
- app1.twake.app (window 1) → Renderer Process A
- app1.twake.app (window 2) → Renderer Process A (same origin = same process by default)
- app2.twake.app (window 3) → Renderer Process B

**If a WebView crashes:**
- Only that window is affected
- Browser process (shell) remains alive
- Sync engine (Rust) remains alive
- User can reload the window

### Bridge: WebView ↔ Rust Sync Engine

**JavaScript API (injected in each WebView):**

```javascript
// twake-client.js — injected by CEF on Twake domains only
class TwakeClient {
  constructor() {
    this.eventHandlers = new Map();
  }

  // Synchronous requests
  async getFileStatus(path) {
    return window.__twake.getFileStatus(path);
  }

  async requestHydration(path) {
    return window.__twake.requestHydration(path);
  }

  // Event subscription (real-time from Rust engine)
  on(event, callback) {
    if (!this.eventHandlers.has(event)) {
      this.eventHandlers.set(event, []);
    }
    this.eventHandlers.get(event).push(callback);
    window.__twake.subscribe(event);
  }

  emit(event, data) {
    window.__twake.emit(event, JSON.stringify(data));
  }
}

export const twake = new TwakeClient();
```

**CEF Side (C++):**

```cpp
// Inject bridge only on trusted Twake domains
void MyRenderDelegate::OnContextCreated(
    CefRefPtr<CefBrowser> browser,
    CefRefPtr<CefFrame> frame,
    CefRefPtr<CefV8Context> context) {
  
  std::string url = frame->GetURL();
  if (!IsTwakeDomain(url)) {
    return; // Don't inject on non-Twake domains
  }

  CefRefPtr<CefV8Value> twake = CefV8Value::CreateObject();

  // Register synchronous methods
  twake->SetValue("getFileStatus",
    CefV8Handler::Create([this](const CefV8ValueList& args) {
      std::string path = args[0]->GetStringValue();
      FileStatus status = sync_engine_->GetFileStatus(path);
      return ConvertToV8(status);
    }),
    V8_PROPERTY_ATTRIBUTE_READONLY);

  twake->SetValue("emit",
    CefV8Handler::Create([this](const CefV8ValueList& args) {
      std::string event = args[0]->GetStringValue();
      std::string data = args[1]->GetStringValue();
      sync_engine_->EmitEvent(event, data);
      return true;
    }),
    V8_PROPERTY_ATTRIBUTE_READONLY);

  context->GetGlobal()->SetValue("__twake", twake, V8_PROPERTY_ATTRIBUTE_NONE);
}

bool MyRenderDelegate::OnProcessMessageReceived(
    CefRefPtr<CefBrowser> browser,
    CefRefPtr<CefFrame> frame,
    CefProcessId source_process,
    CefRefPtr<CefProcessMessage> message) {
  
  // Handle messages from Rust engine (events)
  if (message->GetName() == "TWAKE_EVENT") {
    std::string event = message->GetArgumentList()->GetString(0);
    std::string data = message->GetArgumentList()->GetString(1);
    // Dispatch to subscribed handlers in JS
    DispatchJSEvent(browser, event, data);
    return true;
  }

  return false;
}
```

**Security:** Bridge is only injected on trusted Twake domains. Verified in `OnContextCreated` by checking the URL.

---

## Sync Engine: Rust Architecture

### Core Components

```
Sync Engine (Rust)
│
├── VFS Layer
│   ├── Linux: FUSE (fuse3 crate)
│   ├── Windows: ProjFS (projfs crate)
│   └── macOS: FileProvider (FFI to native framework)
│
├── Reconciliation Engine
│   ├── Phase 1: CouchDB/PouchDB (last-write-wins + backup)
│   └── Phase 2: Yjs/CRDT (deterministic merge)
│
├── Network Layer
│   ├── WebSocket (tokio-tungstenite)
│   ├── SSE (reqwest)
│   └── HTTP fallback (reqwest)
│
├── Event Bus
│   └── tokio::sync::broadcast (internal)
│
└── IPC Server
    └── jsonrpsee (Unix socket / named pipe)
```

### VFS Layer: Platform-Specific Implementation

**This is the most structuring architectural choice.**

| Platform | API | Crate | Status |
|----------|-----|-------|--------|
| **Linux** | FUSE | `fuse3` | ✅ Mature, well-documented |
| **Windows** | ProjFS (Cloud Files API) | `projfs` | ✅ Mature, used by OneDrive |
| **macOS** | FileProvider | Native (FFI) | ✅ Official, used by Dropbox |

**Node state tracking:**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FileState {
    Ghost,        // Metadata only, content not downloaded
    Hydrated,     // Content on disk, in sync
    Modified,     // Local changes pending sync
    Syncing,      // In progress
    Conflict,     // Conflict detected
    Error,        // Sync error
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub id: Uuid,           // Immutable UUID (true identity)
    pub path: PathBuf,      // Current path (alias, can change)
    pub state: FileState,
    pub size: u64,
    pub modified: SystemTime,
    pub version: Version,
    pub parent_id: Option<Uuid>,
}
```

**Key insight:** UUID is the true identity; path is just an alias. Essential for handling renames during editing.

### Reconciliation Engine: Phased Approach

**Phase 1: CouchDB/PouchDB (MVP)**

```rust
pub trait ReconciliationEngine {
    fn merge(&self, local: Document, remote: Document) -> Result<MergeResult, ReconciliationError>;
    fn track_changes(&self, doc: Document) -> Result<ChangeSet, ReconciliationError>;
    fn detect_conflicts(&self, local_rev: &Revision, remote_rev: &Revision) -> bool;
}

pub struct CouchStyleEngine {
    database: PouchDB,
}

impl ReconciliationEngine for CouchStyleEngine {
    fn merge(&self, local: Document, remote: Document) -> Result<MergeResult> {
        // Last-write-wins with backup
        if local.version > remote.version {
            Ok(MergeResult::LocalWins(local))
        } else if remote.version > local.version {
            Ok(MergeResult::RemoteWins(remote))
        } else {
            // Conflict: create backup, let user choose
            self.create_backup(&local)?;
            Ok(MergeResult::Conflict { local, remote })
        }
    }
}
```

**Conflict resolution strategy (Phase 1):**
- "Last write wins" with automatic backup copy
- User sees conflict UI, can choose version
- Like Git merge tool
- Honest, no silent data loss

**Phase 2: Yjs/CRDT (when backend ready)**

```rust
pub struct YjsEngine {
    doc: YjsDocument,
}

impl ReconciliationEngine for YjsEngine {
    fn merge(&self, local: Document, remote: Document) -> Result<MergeResult> {
        // CRDT merge: deterministic, no data loss
        let merged = self.doc.merge_ops(local.ops, remote.ops);
        Ok(MergeResult::Merged(merged))
    }
}
```

**Migration path:**

```rust
// Abstract trait from the start
pub enum ReconciliationBackend {
    Couch(CouchStyleEngine),
    Yjs(YjsEngine),
}

impl ReconciliationEngine for ReconciliationBackend {
    fn merge(&self, local: Document, remote: Document) -> Result<MergeResult> {
        match self {
            ReconciliationBackend::Couch(engine) => engine.merge(local, remote),
            ReconciliationBackend::Yjs(engine) => engine.merge(local, remote),
        }
    }
}
```

**2 days of design that save 6 months of pain later.**

### Event Bus: Inter-WebView Communication

**The Problem:**
Twake web apps are on different origins (app1.twake.app, app2.twake.app, etc.). CEF/Chromium behavior:
- Each origin gets separate renderer processes
- SharedWorkers do NOT work across origins
- postMessage does NOT work natively between WebViews
- BroadcastChannel does NOT work across origins

**Solution: Rust Sync Engine as Event Bus**

```
app1.twake.app ──► window.__twake.emit('file.changed') ──┐
app2.twake.app ──► window.__twake.emit('file.changed') ──┼──► Sync Engine
app3.twake.app ──► window.__twake.emit('file.changed') ──┘      (event bus)
                                                                  │
                                                                  ▼
                                                            window.__twake.on('file.changed', ...)
```

**Rust implementation:**

```rust
use tokio::sync::broadcast;
use serde_json::Value;

pub enum TwakeEvent {
    FileChanged { path: PathBuf, status: FileStatus },
    SyncStarted { path: PathBuf },
    SyncCompleted { path: PathBuf },
    Notification { app: String, payload: NotificationPayload },
    Conflict { path: PathBuf, versions: Vec<Version> },
}

pub struct EventBus {
    tx: broadcast::Sender<TwakeEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel::<TwakeEvent>(256);
        Self { tx }
    }

    pub async fn publish(&self, event: TwakeEvent) -> Result<(), broadcast::error::SendError<TwakeEvent>> {
        self.tx.send(event)?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TwakeEvent> {
        self.tx.subscribe()
    }

    // For IPC: send events to CEF shell
    pub async fn forward_to_shell(&self, event: &TwakeEvent, shell_tx: &mpsc::Sender<ShellEvent>) {
        let shell_event = ShellEvent::FromRust(event.clone());
        shell_tx.send(shell_event).await.ok();
    }
}

// Internal event loop
pub async fn event_loop(
    mut internal_rx: broadcast::Receiver<TwakeEvent>,
    shell_tx: mpsc::Sender<ShellEvent>,
) {
    while let Ok(event) = internal_rx.recv().await {
        // Forward to CEF shell for JS dispatch
        forward_to_shell(&event, &shell_tx).await;
        
        // Also handle internally (notifications, sync triggers, etc.)
        handle_internal(&event).await;
    }
}
```

**CEF Shell receives events and dispatches to JS:**

```cpp
// In C++ shell, listen for events from Rust
async fn shell_event_loop(
    mut rust_rx: mpsc::Receiver<ShellEvent>,
    browsers: HashMap<BrowserId, CefRefPtr<CefBrowser>>,
) {
    while let Ok(event) = rust_rx.recv().await {
        match event {
            ShellEvent::FromRust(TwakeEvent::FileChanged { path, status }) => {
                // Dispatch to all subscribed WebViews
                for browser in browsers.values() {
                    let message = CefProcessMessage::Create("TWAKE_EVENT");
                    message->GetArgumentList()->SetString(0, "file.changed");
                    message->GetArgumentList()->SetString(1, format!("{{\"path\":\"{}\",\"status\":\"{}\"}}", path, status));
                    browser->GetMainFrame()->SendProcessMessage(PID_RENDERER, message);
                }
            }
            // ... other events
        }
    }
}
```

### Network Layer

**WebSocket for real-time CRDT ops (Phase 2):**
- Persistent connection to Twake server
- Protocol: y-websocket or custom JSON-RPC
- Server acts as CRDT relay, not truth arbitrator

**Polling fallback (Phase 1):**
- CouchDB replication over HTTP
- Polling for changes when WebSocket unavailable

**IPC to CEF Shell:**

```rust
// Using jsonrpsee for JSON-RPC over Unix socket
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;

#[rpc(server, client)]
pub trait TwakeSyncApi {
    #[method(name = "file.status")]
    async fn file_status(&self, path: String) -> RpcResult<FileStatus>;
    
    #[method(name = "file.hydrate")]
    async fn file_hydrate(&self, path: String) -> RpcResult<()>;
    
    #[subscription(name = "events.subscribe", item = TwakeEvent)]
    async fn subscribe_events(&self) -> SubscriptionResult;
    
    #[method(name = "events.emit")]
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()>;
}

pub struct SyncEngineApi {
    engine: Arc<SyncEngine>,
}

#[async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    async fn file_status(&self, path: String) -> RpcResult<FileStatus> {
        Ok(self.engine.get_file_status(&path).await)
    }

    async fn file_hydrate(&self, path: String) -> RpcResult<()> {
        self.engine.hydrate_file(&path).await?;
        Ok(())
    }

    async fn subscribe_events(&self) -> SubscriptionResult {
        // Return event stream
        Ok(self.event_bus.subscribe())
    }

    async fn emit_event(&self, event: String, data: String) -> RpcResult<()> {
        self.event_bus.emit(event, data).await?;
        Ok(())
    }
}
```

---

## Technology Stack

### CEF Shell (C++)

| Component | Technology | Notes |
|-----------|------------|-------|
| **CEF** | Chromium Embedded Framework | Latest stable branch |
| **Build** | CMake + CEF binaries | Prebuilt from Spotify CDN |
| **Window management** | Native OS APIs | Win32 / Cocoa / GTK |
| **Tray icon** | Native OS APIs | Win32 / NSStatusItem / AppIndicator |
| **Notifications** | Native OS APIs | WinRT / NSUserNotification / libnotify |
| **IPC client** | JSON-RPC over Unix socket | `cpp-ipc` or custom |

### Sync Engine (Rust)

| Component | Crate | Notes |
|-----------|-------|-------|
| **Async runtime** | `tokio` | Multi-threaded, full featured |
| **VFS Linux** | `fuse3` | FUSE 3.x bindings |
| **VFS Windows** | `projfs` | ProjFS bindings |
| **VFS macOS** | FFI to FileProvider | Native framework |
| **Database** | `sqlx` / `rusqlite` | Async SQL, migrations |
| **Reconciliation** | `pouchdb-rs` (Phase 1) | CouchDB client |
| **Reconciliation** | `y-crdt` (Phase 2) | CRDT implementation |
| **WebSocket** | `tokio-tungstenite` | Async WebSocket |
| **HTTP** | `reqwest` | Async HTTP client |
| **IPC server** | `jsonrpsee` | JSON-RPC over Unix socket |
| **Event bus** | `tokio::sync::broadcast` | Multi-producer, multi-consumer |
| **UUID** | `uuid` | Version 4, fast |

---

## Critical Design Decisions

### 1. Two-Process Architecture

**Decision:** Separate CEF shell (C++) and sync engine (Rust) processes.

**Rationale:**
- Isolation: WebView crash ≠ sync engine crash
- Security: Minimal C++ surface, Rust sandboxed
- Performance: Separate CPU cores
- Maintainability: Clear separation (UI vs. business logic)

### 2. UUID-Based File Identity

**Decision:** Every file has an immutable UUID; path is just an alias.

**Rationale:**
- Renames during editing are manageable
- Many sync clients failed on this point
- Essential for conflict resolution

### 3. ReconciliationEngine Abstraction

**Decision:** Define trait from the start, implement CouchStyleEngine first.

**Rationale:**
- Don't over-engineer Phase 1
- Don't close doors to Phase 2
- 2 days of design save 6 months of pain

### 4. CEF over Electron/Tauri

**Decision:** CEF for production, Electron acceptable for MVP.

**Rationale:**
- No Node.js exposure (security)
- Lighter RAM footprint
- Same Chromium rendering
- C++ shell is minimal (500-1000 lines)

### 5. Rust Event Bus for Inter-WebView Communication

**Decision:** All inter-WebView communication goes through Rust engine.

**Rationale:**
- SharedWorkers impossible across origins
- Centralized, testable logic
- Decouples WebViews from each other

---

## Risks and Mitigations

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| **VFS complexity (3 APIs)** | High | High | Start with Linux/FUSE, prove concept |
| **C++ skill gap** | High | Medium | Already in team; 500-1000 lines only |
| **CEF build complexity** | Medium | High | Use prebuilt binaries, document setup |
| **WebView crash isolation** | Medium | Low | Force one renderer process per window |
| **Inter-WebView communication** | High | Certain | Rust event bus (no SharedWorkers) |
| **Reconciliation backend lock-in** | High | Medium | ReconciliationEngine trait from start |
| **Yjs backend compatibility** | High | Medium | Phase 1 with CouchDB, abstraction for Phase 2 |
| **macOS FileProvider notarization** | Medium | High | Start early with Apple developer account |
| **IPC reliability** | Medium | Low | jsonrpsee is mature, well-tested |

---

## Development Phases

### Phase 1: POC (2-3 weeks)

**Goals:**
- CEF shell setup (CMake, prebuilt binaries)
- Basic window management
- Bridge JS ↔ C++ (window.__twake)
- IPC: C++ ↔ Rust (Unix socket)

**Deliverables:**
- Working POC on one platform (Linux)
- WebView can call Rust engine
- Rust engine can send events to WebView

### Phase 2: MVP (3 months)

**Goals:**
- OIDC PKCE authentication
- VFS on one platform (Linux/FUSE)
- CouchDB-style reconciliation (last-write-wins + backup)
- One web app in native window

**Deliverables:**
- Working prototype on Linux
- Authentication flow
- File sync with placeholder support
- One web app (e.g., Drive) in native window

### Phase 3: Production Ready (6 months)

**Goals:**
- Full cross-platform VFS (Windows ProjFS, macOS FileProvider)
- Conflict resolution UI
- Multiple web apps (Drive, Mail, Calendar, Chat)
- Unified notifications
- Auto-update infrastructure

**Deliverables:**
- Production build on all 3 platforms
- Full feature set
- Performance optimization
- Packaging (deb, rpm, .msi, .dmg)

### Phase 4: CRDT Migration (6-12 months)

**Goals:**
- Backend support for Yjs
- Migrate reconciliation engine to CRDT
- Real-time collaboration features

**Deliverables:**
- Deterministic merge for simultaneous edits
- Offline collaboration support
- Yjs integration with OnlyOffice

---

## Implementation Checklist

### CEF Shell (C++)

- [ ] Setup CMake build with CEF prebuilt binaries
- [ ] Implement window management (create, close, minimize, maximize)
- [ ] Implement tray icon (Windows/Mac/Linux)
- [ ] Implement native notifications (WinRT/NSUserNotification/libnotify)
- [ ] Implement bridge injection (only on Twake domains)
- [ ] Implement IPC client (JSON-RPC to Rust)
- [ ] Implement event dispatch (Rust → JS)
- [ ] Handle renderer crash recovery
- [ ] Setup auto-update mechanism

### Sync Engine (Rust)

- [ ] Setup tokio async runtime
- [ ] Implement VFS trait (common interface)
- [ ] Implement FUSE backend (Linux)
- [ ] Implement ProjFS backend (Windows)
- [ ] Implement FileProvider backend (macOS)
- [ ] Implement FileNode model (UUID, path, state)
- [ ] Implement ReconciliationEngine trait
- [ ] Implement CouchStyleEngine (Phase 1)
- [ ] Implement jsonrpsee server (IPC)
- [ ] Implement event bus (tokio::broadcast)
- [ ] Implement WebSocket client (server sync)
- [ ] Implement OIDC PKCE client
- [ ] Implement token storage (keyring/keystore)

### Integration

- [ ] Define IPC protocol (JSON-RPC schema)
- [ ] Implement event types (TwakeEvent enum)
- [ ] Test cross-platform VFS
- [ ] Test inter-WebView communication
- [ ] Test conflict resolution UI
- [ ] Test auto-update on all platforms

---

## References

### Core Technologies

- [CEF - Chromium Embedded Framework](https://bitbucket.org/chromiumembedded/cef)
- [Electron](https://www.electronjs.org/)
- [Tauri - Desktop framework](https://tauri.app/)
- [Yjs - Shared editing data structure](https://yjs.dev/)
- [Automerge - CRDT data structure](https://automerge.org/)
- [CouchDB - Replication protocol](https://cloudant.com/blog/couchdb-replication-explained)
- [PouchDB - Client-side database](https://pouchdb.com/)

### VFS Platforms

- [ProjFS - Windows Virtual File System](https://docs.microsoft.com/en-us/windows/win32/fileio/projected-file-system)
- [FileProvider - macOS VFS](https://developer.apple.com/documentation/fileprovider)
- [FUSE - Linux VFS](https://github.com/libfuse/libfuse)

### Rust Crates

- [tokio](https://tokio.rs/) — Async runtime
- [fuse3](https://crates.io/crates/fuse3) — FUSE bindings
- [projfs](https://crates.io/crates/projfs) — ProjFS bindings (Windows)
- [sqlx](https://github.com/launchbadge/sqlx) — Async SQL
- [jsonrpsee](https://github.com/paritytech/jsonrpsee) — JSON-RPC
- [uuid](https://github.com/uuid-rs/uuid) — UUID support
- [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) — WebSocket
- [reqwest](https://github.com/seanmonstar/reqwest) — HTTP client

### C++ Libraries

- [CEF](https://bitbucket.org/chromiumembedded/cef) — Chromium Embedded Framework
- [cpp-ipc](https://github.com/luqmana/cppipc) — C++ IPC library (optional)
- [libnotify](https://gitlab.gnome.org/GNOME/libnotify) — Linux notifications
- [windows](https://github.com/microsoft/windows-rs) — Windows API (can use C++ too)

### Patterns

- [Syncthing - Conflict resolution approach](https://syncthing.net/)
- [OneDrive - Files On-Demand](https://support.microsoft.com/en-us/office/get-started-with-files-on-demand-in-onedrive-7462773f-8e54-47f2-98b1-c9c5e2a83e94)
- [Dropbox - Smart Sync](https://www.dropbox.com/smart-sync)
- [Git merge patterns - Manual conflict resolution](https://git-scm.com/book/en/v2/Git-Branching-Branch-Management)

---

## Appendix: Architecture Decision Record

### ADR-001: Two-Process Architecture (CEF + Rust)

**Status:** Accepted

**Context:**
Need a desktop shell to host Twake web apps and a sync engine for VFS/reconciliation. Multiple technology options available.

**Decision:**
- Shell: CEF (C++) for web rendering
- Sync Engine: Rust for VFS, reconciliation, network
- Communication: JSON-RPC over Unix socket / named pipe

**Rationale:**
1. CEF provides Chromium rendering (identical to Chrome)
2. No Node.js exposure (security)
3. Rust provides memory safety and performance for sync engine
4. Process isolation (crash resilience)
5. Clear separation of concerns (UI vs. business logic)

**Consequences:**
- C++ required for shell (500-1000 lines, manageable)
- IPC adds complexity but enables isolation
- Build system more complex (CMake + Cargo)
- Team needs C++ skills (already available)

**Alternatives considered:**
- Electron: Too heavy, Node.js security surface
- Tauri: WebView system rendering variations
- CEF + Go: Bindings immature
- CEF + C++: More complex, no Rust benefits

---

*Document version: 4.0*  
*Last updated: 2026-03-25*  
*Status: Final — Ready for development*
