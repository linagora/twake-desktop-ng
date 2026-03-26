# ADR-0003: Two-Process Architecture (CEF Shell + Rust Sync Engine)

## Status

Superseded by [ADR-0004](ADR-0004-electron-migration.md) — CEF replaced by Electron; two-process architecture retained.

## Context

Twake Desktop NG requires a desktop application that:

1. Hosts multiple Twake web applications (Drive, Mail, Calendar, etc.) in native windows
2. Provides a Virtual File System (VFS) with on-demand file hydration
3. Synchronizes files between local VFS and remote Twake server
4. Enables real-time collaboration with conflict reconciliation
5. Maintains security and isolation between components

Technology choices considered:

- Electron (Chromium + Node.js)
- Tauri (WebView + Rust)
- CEF (Chromium Embedded Framework) + Rust
- CEF + C++

## Decision

Adopt a **two-process architecture**:

1. **CEF Shell (C++)** - ~100MB RAM
   - Hosts multiple WebViews for Twake web apps
   - Window management (create, close, minimize, maximize)
   - Native integration (tray icon, OS notifications)
   - JavaScript bridge (`window.__twake`)
   - IPC client to communicate with sync engine

2. **Sync Engine (Rust)** - ~30MB RAM
   - VFS implementation (FUSE on Linux, ProjFS on Windows, FileProvider on macOS)
   - File synchronization and reconciliation
   - Conflict detection and resolution
   - Network layer (WebSocket, SSE, HTTP)
   - Authentication (OIDC PKCE)
   - Event bus for inter-component communication

**Communication:** JSON-RPC over Unix socket (Linux/macOS) / named pipe (Windows)

## Rationale

### Why Two Processes?

1. **Isolation** - WebView crash ≠ sync engine crash
2. **Security** - Minimal C++ surface, Rust sandboxed
3. **Performance** - Separate CPU cores, no contention
4. **Maintainability** - Clear separation (UI vs. business logic)
5. **Team parallelism** - C++ team and Rust team work independently

### Why CEF over Electron/Tauri?

| Criteria                   | CEF           | Electron    | Tauri             |
| -------------------------- | ------------- | ----------- | ----------------- |
| Web rendering              | ✅ Chromium   | ✅ Chromium | ⚠️ System WebView |
| Install size               | ~100MB        | ~150MB      | ~10MB             |
| RAM idle                   | ~100-120MB    | ~200-300MB  | ~30MB             |
| Node.js exposure           | ✅ No         | ⚠️ Yes      | ✅ No             |
| Security                   | High          | Medium      | High              |
| Cross-platform consistency | ✅ Guaranteed | ✅ Good     | ⚠️ Varies         |
| C++ shell complexity       | Moderate      | N/A         | N/A               |

**Decision: CEF**

**Rationale:**

1. **Guaranteed rendering** - Bundled Chromium, identical to Chrome
2. **Security** - No Node.js exposure, reduced attack surface
3. **Stability** - 17 years of existence, used by Adobe, Spotify, Riot Games
4. **Process isolation** - Renderer processes are isolated
5. **Trade-off acceptable** - C++ shell is minimal (500-1000 lines)

### Why Rust for Sync Engine?

1. **Memory safety** - No garbage collection, no data races
2. **Performance** - Native code, async runtime (tokio)
3. **VFS libraries** - Mature crates (fuse3, projfs)
4. **Network stack** - Excellent async HTTP/WebSocket support
5. **Cross-platform** - Single codebase for all 3 platforms

## Architecture Diagram

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
└─────────────────────────────────────────────────────────────┘
```

## Consequences

### Positive

1. **Crash resilience** - Each process can fail independently
2. **Security boundaries** - CEF sandbox + Rust memory safety
3. **Team scalability** - C++ and Rust teams work in parallel
4. **Testability** - Each process can be tested independently
5. **Performance** - No contention between UI and sync operations

### Negative

1. **IPC complexity** - Must serialize/deserialize all communication
2. **Build complexity** - Two build systems (CMake + Cargo)
3. **Debugging** - Cross-process debugging more complex
4. **C++ requirement** - Need C++ skills for shell (500-1000 lines)

### Risks

1. **IPC reliability** - Socket disconnections, race conditions
2. **Build complexity** - CEF prebuilt binaries, CMake configuration
3. **C++ skill gap** - Team needs C++ expertise (already available)
4. **Cross-platform VFS** - 3 different APIs (FUSE, ProjFS, FileProvider)

## Migration Path

**Phase 1: POC (2-3 weeks)**

- CEF shell setup with basic window management
- IPC contract definition (JSON-RPC schema)
- Rust sync engine skeleton with in-memory VFS
- End-to-end test: WebView → IPC → VFS → response

**Phase 2: MVP (3 months)**

- Full VFS implementation on Linux (FUSE)
- OIDC authentication
- File hydration (download + write)
- One web app (Drive) in native window

**Phase 3: Production (6 months)**

- Windows (ProjFS) and macOS (FileProvider) support
- Conflict resolution UI
- Multiple web apps
- Auto-update infrastructure

**Phase 4: CRDT (6-12 months)**

- Migrate reconciliation engine to Yjs/CRDT
- Real-time collaborative editing
- Offline collaboration support

## References

- [CEF - Chromium Embedded Framework](https://bitbucket.org/chromiumembedded/cef)
- [Electron](https://www.electronjs.org/)
- [Tauri](https://tauri.app/)
- [docs/spec.md](../spec.md) - Full technical specification
- [docs/superpowers/specs/cef-shell-design.md](../superpowers/specs/cef-shell-design.md) - CEF Shell detailed spec
- [docs/superpowers/specs/vfs-engine-design.md](../superpowers/specs/vfs-engine-design.md) - VFS Engine detailed spec
