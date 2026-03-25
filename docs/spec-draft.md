# Twake Desktop NG - Technical Specification

**Version:** 2.0
**Date:** 2026-03-25
**Author:** Ben + Technical Review

---

## Project Objective

Develop a new desktop client for Twake that provides an enriched desktop experience for Twake web services.

This is **not just file synchronization** — it's a **collaborative work platform** with multi-actor support, enabling:

- Local editing (appears local when connected) for some users
- Remote editing via Twake web UI (OnlyOffice) for others
- All collaborating on the same document
- Full offline support with reconciliation on reconnection
- Partial remote tree exposure (bandwidth optimization)

---

## Key Features

### Virtual File System (VFS)

- Exposure of synchronized file system via virtual file system
- **Placeholder files:** Non-downloaded files exist as "ghosts" in filesystem
- **On-demand hydration:** First access triggers download
- **Partial sync:** Users can choose which parts of remote tree to keep locally

### Web Services on Desktop

- Opening Twake web apps in native windows (browser without address bar)
- Requires WebView integration (Tauri recommended)

### Offline and Cache Management

- Offline support with configurable cache strategies for web apps
- Service Worker integration (depends on Twake backend PWA support)
- Local editing with deferred sync on reconnection

### Unified Notification Center

- Centralized notifications for all Twake services (mail, chat, calendar, video)
- Native OS notifications (WinRT, libnotify, NSUserNotification)
- Internal event bus decoupling network reception from UI rendering

### Embedded File Editing

- Integration of editors like OnlyOffice to open VFS files
- **Collaborative mode:** OnlyOffice server (online); local saving (offline)
- **Avoids native Word, Excel, PowerPoint files**
- **CRDT-based reconciliation** for simultaneous editing

---

## Technical Architecture

### Language & Runtime: **Rust**

**Why Rust (no hesitation for sync core):**

- Zero memory crashes by construction
- Native performance, low RAM footprint
- Cross-platform without friction
- Mature ecosystem:
  - `tokio` for async
  - `notify` for filesystem watching
  - `sqlx`/`rusqlite` for local DB
  - `y-crdt`/`automerge-rs` for CRDT

**Avoid:**

- ❌ Electron (too heavy for background sync client)
- ❌ Go (good for network services, but native bindings painful)
- ❌ Java (JVM, heavy packaging, no clean native VFS integration)

### UI Layer: **CEF vs Electron Comparison**

| Aspect             | Electron                | CEF                     | Tauri            |
| ------------------ | ----------------------- | ----------------------- | ---------------- |
| Install size       | ~150MB                  | ~100MB                  | ~10MB            |
| Idle RAM           | ~200-300MB              | ~100-120MB              | ~30MB            |
| WebView            | Chromium bundled        | Chromium bundled        | System WebView   |
| WebApp rendering   | ✅ Identical everywhere | ✅ Identical everywhere | ⚠️ Varies by OS  |
| Node.js exposed    | ⚠️ Yes                  | ✅ No                   | ✅ No            |
| Native VFS backend | ❌ Complicated          | ⚠️ IPC/FFI              | ✅ Rust native   |
| Language shell     | JS/TS                   | C++                     | Rust             |
| Packaging          | ✅ Trivial              | ⚠️ Complex              | ✅ Simple        |
| Maturity           | ✅ Very mature          | ✅ 17 years (stable)    | 🟡 Young         |
| SharedWorkers      | ❌ Cross-WebView        | ❌ Cross-WebView        | ❌ Cross-WebView |

**Decision: CEF (Chromium Embedded Framework)**

**Rationale:**

1. **Guaranteed rendering** — Chromium bundlé, même comportement que Chrome
2. **Security** — Pas de Node.js exposé, surface d'attaque réduite
3. **Stability** — 17 ans d'existence, utilisé par Adobe, Spotify, Riot Games
4. **Process isolation** — Une WebView qui crash n'emporte pas les autres

**Trade-offs:**

- C++ pour le shell (vs JS/TS avec Electron)
- Packaging plus complexe (build system CEF)
- Nécessite des compétences C++ dans l'équipe

**Alternative acceptable:** Electron pour le MVP (livraison rapide), migration vers CEF en Phase 2.

**CEF Configuration:**

```cpp
// Force one renderer process per window
CefSettings settings;
settings.multi_threaded_message_loop = true;
command_line->AppendSwitch("process-per-tab");
```

**Risk:** If a WebView crashes, only that window is affected. The Rust sync engine remains running.

### Sync Engine: **Phased Approach (CouchDB → CRDT)**

#### The Real Problem: Distributed Consistency

Multiple actors writing simultaneously via different paths:

```
User A (local, apparent offline)  ──┐
User B (web, OnlyOffice)          ──┼──► Document "true state"
User C (local, truly offline)     ──┘
```

**Short-term reality:** Yjs requires everyone to speak the same protocol, including the server. If Twake backend isn't ready to evolve quickly, this is a real blocker.

#### Phase 1 — Realistic MVP with CouchDB

Accept current constraints and build something that works:

**CouchDB/PouchDB for tree and metadata reconciliation**

- Multi-master replication built-in
- Document revisions (MVCC)
- Standardized replication protocol
- Conflict detection (not silent overwrite)

**For content conflicts: explicit and assumed strategy**

- "Last write wins" with automatic backup copy of conflicting version
- User sees conflict, can choose — like Git with merge tool
- Not elegant, but honest and no data loss
- This is what Syncthing does, and people adapt well

**Reconciliation Engine Abstraction (critical for Phase 2):**

```rust
trait ReconciliationEngine {
    fn merge(&self, local: Document, remote: Document) -> MergeResult;
    fn track_changes(&self, doc: Document) -> ChangeSet;
}

// Implementation V1
struct CouchStyleEngine { ... }

// Implementation V2 (later)
struct YjsEngine { ... }
```

If this abstraction is clean from the start, swapping the reconciliation engine doesn't require a rewrite. **2 days of design that save 6 months of pain later.**

#### Phase 2 — CRDT Migration When Backend Catches Up

**Yjs / y-crdt as pivot protocol between OnlyOffice, desktop client, and server**

- Conflict-free merge of simultaneous edits
- Deterministic reconciliation
- Native OnlyOffice integration (Yjs already embedded)
- **WebSocket** for real-time CRDT op propagation (y-websocket / hocuspocus)
- **Server acts as CRDT relay**, not truth arbitrator
- **The CRDT itself is the distributed truth**

#### File Identity

- **Immutable UUID per file/node** — path is just an alias
- Many sync clients failed on this point
- Essential for handling renames during editing

### VFS Implementation

**This is the most structuring architectural choice** — under-estimated in initial spec.

| Platform | API                          | Status                                        |
| -------- | ---------------------------- | --------------------------------------------- |
| Linux    | FUSE                         | Mature, well-supported                        |
| macOS    | FUSE-T or Apple FileProvider | Official API since macOS 12 (Dropbox uses it) |
| Windows  | Cloud Files API (ProjFS)     | What OneDrive uses                            |

**These three APIs are radically different.** Not "we make a VFS and it works everywhere." Three distinct integrations.

**Node state tracking:**

```rust
Node = {
    path: String,
    state: Ghost | Local | Modified | InConflict | Synced,
    vectorClock: VectorClock,
    crdtState: CRDTState
}
```

### Inter-WebView Communication

**The Problem:**
Twake web apps are on different origins (app1.twake.app, app2.twake.app, etc.). CEF/Chromium behavior:

- Each origin gets separate renderer processes
- SharedWorkers do NOT work across origins
- postMessage does NOT work natively between WebViews
- BroadcastChannel does NOT work across origins

**Solution: Rust Démon as Event Bus**

```
app1.twake.app ──► window.__twake.emit('file.changed') ──┐
app2.twake.app ──► window.__twake.emit('file.changed') ──┼──► Rust Démon
app3.twake.app ──► window.__twake.emit('file.changed') ──┘      (event bus)
                                                                  │
                                                                  ▼
                                                            window.__twake.on('file.changed', ...)
```

**JavaScript client library (injected in each WebView):**

```javascript
// twake-client.js
class TwakeClient {
  emit(event, data) {
    window.__twake.emit(event, JSON.stringify(data));
  }

  on(event, callback) {
    window.__twake.on(event, (e, d) => callback(JSON.parse(d)));
  }
}
```

**CEF Bridge (C++):**

```cpp
void InjectTwakeBridge(CefRefPtr<CefV8Context> context) {
  CefRefPtr<CefV8Value> twake = CefV8Value::CreateObject();
  twake->SetValue("emit", CefV8Handler::Create(/* callback to Rust */), V8_PROPERTY_ATTRIBUTE_READONLY);
  twake->SetValue("on", CefV8Handler::Create(/* callback to Rust */), V8_PROPERTY_ATTRIBUTE_READONLY);
  context->GetGlobal()->SetValue("__twake", twake, V8_PROPERTY_ATTRIBUTE_NONE);
}
```

**Security:** Only inject bridge on trusted Twake domains in `OnContextCreated`.

### Notification Architecture

**Two clear layers:**

**1. Transport:** Persistent WebSocket or SSE for real-time server events

**2. Local Dispatch:** Native OS notifications via `notify-rust` (Linux), WinRT (Windows), NSUserNotification (macOS)

**Internal event bus:** `tokio::sync::broadcast` decoupling network reception from UI rendering

```rust
pub enum TwakeEvent {
    FileChanged { path: PathBuf, status: FileStatus },
    SyncStarted { path: PathBuf },
    SyncCompleted { path: PathBuf },
    Notification { app: String, payload: NotificationPayload },
    Conflict { path: PathBuf, versions: Vec<Version> },
}

let (bus_tx, bus_rx) = tokio::sync::broadcast::channel::<TwakeEvent>(256);
```

---

## Technical Advantages

| Aspect             | Benefit                                                             |
| ------------------ | ------------------------------------------------------------------- |
| Virtual System     | Better conflict management; simplified file events; optimized cache |
| Offline Mode       | Continuous usage without network; deferred sync                     |
| Editor Integration | Native productivity; smooth collaboration                           |
| Notifications      | Unified experience, without multiple trays                          |
| CRDT               | Deterministic merge, no data loss from concurrent edits             |
| Tauri + Rust       | 10MB install, 30MB RAM, memory safety guarantees                    |

---

## MVP

1. ✅ Can authenticate (OIDC PKCE flow)
2. ✅ Can synchronize a local directory with the remote one (VFS with placeholders)
3. ✅ Can open a web app in a local window (online for now)

---

## First Steps

### 1. Technology Decision (DONE)

- **Language:** Rust
- **UI Framework:** CEF (Chromium Embedded Framework)
  - Alternative: Electron for MVP, migrate to CEF in Phase 2
- **Shell Language:** C++ (minimal, window management + bridge only)
- **Metadata DB:** CouchDB/PouchDB (Phase 1), SQLite possible
- **Reconciliation:** CouchStyleEngine (Phase 1), YjsEngine (Phase 2)
- **VFS:** Platform-specific (FUSE / FileProvider / ProjFS)
- **IPC:** JSON-RPC over Unix socket / named pipe (`jsonrpsee`)

### 2. Design Docs Required

**Before specification/dev brief:**

- [ ] **VFS Design Doc** — 3 platforms, APIs used, known limitations
- [ ] **ReconciliationEngine Trait** — abstraction for Phase 1→2 migration
- [ ] **Sync Algorithm Design Doc** — nominal cases, error cases, CouchDB strategy
- [ ] **WebApp Compatibility Matrix** — have Twake apps been tested on WebKit?
- [ ] **Server CRDT Protocol Spec** — can Twake backend evolve to Yjs? (Phase 2)

### 3. Packaging

- [ ] Linux: AppImage / Flatpak / deb
- [ ] macOS: .dmg + notarization
- [ ] Windows: .msi + code signing

---

## Critical Risks

| Risk                                  | Mitigation                                        |
| ------------------------------------- | ------------------------------------------------- |
| VFS complexity (3 different APIs)     | Start with one platform, prove concept            |
| C++ skill gap (CEF shell)             | Hire C++ dev or use Electron for MVP              |
| CEF build complexity                  | Use prebuilt CEF binaries, document setup         |
| WebView crash isolation               | Force one renderer process per window             |
| Inter-WebView communication           | Rust event bus (no SharedWorkers)                 |
| CRDT server compatibility (Phase 2)   | Build ReconciliationEngine abstraction in Phase 1 |
| Rename during editing                 | UUID-based identity, not path-based               |
| Offline for extended periods          | CouchDB MVCC handles this                         |
| Content conflict resolution (Phase 1) | "Last write wins" + backup copy + manual choice   |
| OnlyOffice Yjs integration (Phase 2)  | Leverage native OnlyOffice + Yjs integration      |

---

## What to Avoid

- ❌ Go — native bindings (VFS, OS notifications) painful
- ❌ Java — JVM, heavy packaging, no clean native VFS
- ❌ Naive polling sync — battery and performance killer
- ❌ Path-based file identity — use UUIDs
- ❌ Hard-coding CouchDB without abstraction — plan for Phase 2 migration
- ❌ Relying on SharedWorkers/postMessage cross-WebView — use Rust event bus
- ⚠️ Last-write-wins without backup — acceptable in Phase 1 ONLY with backup copy + manual choice
- ⚠️ Electron for production — acceptable for MVP only (RAM, Node.js security surface)

---

## Summary

**Initial spec was good vision, good MVP priorities** — but still at "what" level. Need to descend to "how" on the two hardest subjects:

1. **VFS** — 3 platforms, 3 APIs, 3 implementations
2. **Reconciliation Engine** — CouchDB (Phase 1), CRDT (Phase 2)
3. **Shell Layer** — CEF (production), Electron (MVP acceptable)

**Architecture now defined:**

- **Rust** for sync engine (no hesitation)
- **CEF** for shell (Chromium rendering, no Node.js exposure)
- **CouchDB/PouchDB** for metadata + content (Phase 1)
- **ReconciliationEngine trait** — abstraction for Phase 1→2 migration
- **Yjs CRDT** for document content (Phase 2, when backend ready)
- **VFS with placeholders** for partial sync
- **UUID identity** for files
- **Rust event bus** for inter-WebView communication

**Key advice:**

- Don't over-engineer Phase 1, but don't close doors to Phase 2
- The ReconciliationEngine abstraction is 2 days of design that save 6 months of pain
- Two classic errors to avoid:
  - ❌ Implementing Yjs now without backend ready — over-engineered never-delivered product
  - ❌ Hard-coding CouchDB without abstraction — painful rewrite in 18 months

**If the reconciliation abstraction is clean from the start, swapping engines doesn't require a rewrite.**

**CEF vs Electron decision:**

- **Short-term (MVP, 0-6 months):** Electron acceptable for speed
- **Long-term (production, 12+ months):** CEF for security, performance, stability
- **If C++ team available:** CEF from the start
- **If JS team only:** Electron MVP → CEF migration

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

### Patterns

- [Syncthing - Conflict resolution approach](https://syncthing.net/)
- [OneDrive - Files On-Demand](https://support.microsoft.com/en-us/office/get-started-with-files-on-demand-in-onedrive-7462773f-8e54-47f2-98b1-c9c5e2a83e94)
- [Dropbox - Smart Sync](https://www.dropbox.com/smart-sync)
- [Git merge patterns - Manual conflict resolution](https://git-scm.com/book/en/v2/Git-Branching-Branch-Management)

---

## Appendix: CEF vs Electron Decision Matrix

| Criterion         | Score (CEF) | Score (Electron) | Winner   |
| ----------------- | ----------- | ---------------- | -------- |
| Rendering quality | 10          | 10               | Tie      |
| Weight/RAM        | 8           | 5                | CEF      |
| Security          | 9           | 6                | CEF      |
| Packaging         | 4           | 10               | Electron |
| Hiring            | 4           | 10               | Electron |
| Rust integration  | 7           | 5                | CEF      |
| Maturity          | 9           | 10               | Electron |
| Crash isolation   | 10          | 8                | CEF      |
| C++ complexity    | 3           | 9                | Electron |

**For MVP with JS team:** Electron  
**For production with C++ team:** CEF  
**For long-term maintainability:** CEF

---

_Document version: 3.0-draft_  
_Last updated: 2026-03-25_  
_Status: Ready for technical review_
