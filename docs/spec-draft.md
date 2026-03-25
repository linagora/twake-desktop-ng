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

### UI Layer: **Tauri**

| Aspect             | Electron                | Tauri           |
| ------------------ | ----------------------- | --------------- |
| Install size       | ~150MB                  | ~10MB           |
| Idle RAM           | ~200MB+                 | ~30MB           |
| WebView            | Chromium bundled        | System WebView  |
| WebApp rendering   | ✅ Identical everywhere | ⚠️ Varies by OS |
| Native VFS backend | ❌ Complicated          | ✅ Rust native  |

**Recommendation:** Tauri + Rust backend. For a client running permanently in background, Electron is a burden.

**Risk to know:** Twake WebApps must be tested on all 3 system WebViews. If they assume Chromium, there will be surprises on macOS (WebKit) and Linux (WebKitGTK).

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

### Notification Architecture

**Two clear layers:**

**1. Transport:** Persistent WebSocket or SSE for real-time server events

**2. Local Dispatch:** Native OS notifications via `notify-rust` or `tauri-plugin-notification`

**Internal event bus:** `tokio::broadcast` decoupling network reception from UI rendering

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
- **UI Framework:** Tauri
- **Metadata DB:** CouchDB/PouchDB (Phase 1), SQLite possible
- **Reconciliation:** CouchStyleEngine (Phase 1), YjsEngine (Phase 2)
- **VFS:** Platform-specific (FUSE / FileProvider / ProjFS)

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
| CRDT server compatibility (Phase 2)   | Build ReconciliationEngine abstraction in Phase 1 |
| WebApp WebView differences            | Test early on WebKit (macOS/Linux)                |
| Rename during editing                 | UUID-based identity, not path-based               |
| Offline for extended periods          | CouchDB MVCC handles this                         |
| Content conflict resolution (Phase 1) | "Last write wins" + backup copy + manual choice   |
| OnlyOffice Yjs integration (Phase 2)  | Leverage native OnlyOffice + Yjs integration      |

---

## What to Avoid

- ❌ Electron — too heavy for permanent background sync client
- ❌ Go — native bindings (VFS, OS notifications) painful
- ❌ Java — JVM, heavy packaging, no clean native VFS
- ❌ Naive polling sync — battery and performance killer
- ❌ Path-based file identity — use UUIDs
- ❌ Hard-coding CouchDB without abstraction — plan for Phase 2 migration
- ⚠️ Last-write-wins without backup — acceptable in Phase 1 ONLY with backup copy + manual choice

---

## Summary

**Initial spec was good vision, good MVP priorities** — but still at "what" level. Need to descend to "how" on the two hardest subjects:

1. **VFS** — 3 platforms, 3 APIs, 3 implementations
2. **Reconciliation Engine** — CouchDB (Phase 1), CRDT (Phase 2)

**Architecture now defined:**

- **Rust + Tauri** for client
- **CouchDB/PouchDB** for metadata + content (Phase 1)
- **ReconciliationEngine trait** — abstraction for Phase 1→2 migration
- **Yjs CRDT** for document content (Phase 2, when backend ready)
- **VFS with placeholders** for partial sync
- **UUID identity** for files

**Key advice:**

- Don't over-engineer Phase 1, but don't close doors to Phase 2
- The ReconciliationEngine abstraction is 2 days of design that save 6 months of pain
- Two classic errors to avoid:
  - ❌ Implementing Yjs now without backend ready — over-engineered never-delivered product
  - ❌ Hard-coding CouchDB without abstraction — painful rewrite in 18 months

**If the reconciliation abstraction is clean from the start, swapping engines doesn't require a rewrite.**

---

## References

### Core Technologies

- [Tauri - Desktop framework](https://tauri.app/)
- [Yjs - Shared editing data structure](https://yjs.dev/)
- [Automerge - CRDT data structure](https://automerge.org/)
- [CouchDB - Replication protocol](https://cloudant.com/blog/couchdb-replication-explained)
- [PouchDB - Client-side database](https://pouchdb.com/)

### VFS Platforms

- [ProjFS - Windows Virtual File System](https://docs.microsoft.com/en-us/windows/win32/fileio/projected-file-system)
- [FileProvider - macOS VFS](https://developer.apple.com/documentation/fileprovider)
- [FUSE - Linux VFS](https://github.com/libfuse/libfuse)

### Patterns

- [Syncthing - Conflict resolution approach](https://syncthing.net/)
- [Git merge patterns - Manual conflict resolution](https://git-scm.com/book/en/v2/Git-Branching-Branch-Management)

---

_Document generated from Ben's notes + technical review. Ready for detailed specification._
