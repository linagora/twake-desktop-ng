# Twake Desktop NG - Technical Specification

**Version:** 6.0
**Date:** 2026-03-26
**Status:** Final

---

## Executive Summary

Twake Desktop NG is a **collaborative work platform** (not just a file sync client) that provides an enriched desktop experience for Twake web services. The architecture combines an **Electron shell (TypeScript)** for window management, local SPA hosting, and native integration with a **Rust sync engine** (sidecar process) for VFS, reconciliation, and network operations.

**Key differentiator:** Multi-actor collaboration with offline support, where users can edit the same document via different interfaces (local VFS editor vs. OnlyOffice web) with automatic reconciliation.

**Architecture:** Two-process model with Electron shell and Rust sync engine communicating via JSON-RPC over Unix socket. See [ADR-0004](adr/ADR-0004-electron-migration.md) for the Electron decision rationale.

---

## Project Objectives

### Primary Goals

1. **Virtual File System (VFS)** -- Expose synchronized file system with placeholder files (on-demand hydration)
2. **Web Services on Desktop** -- Open Twake web apps (local SPAs and remote apps) in native windows
3. **Offline-first** -- Full offline support with deferred sync and conflict reconciliation
4. **Unified Notifications** -- Centralized notifications for all Twake services
5. **Embedded Editing** -- Integrate editors (OnlyOffice) for VFS files with collaborative mode

### MVP Scope

1. Authenticate via OIDC PKCE, retrieve an access token
2. Open a local SPA in a secure BrowserWindow, with `window.__twake` bridge
3. SPA can interact with the Electron shell (get token, call sync engine)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│               Electron Shell (TypeScript)                    │
│                                                             │
│  ┌─────────────────────┐   ┌─────────────────────┐         │
│  │    Main Process      │   │  Renderer Processes  │         │
│  │                      │   │  (sandboxed)         │         │
│  │  - BrowserWindow     │   │  ┌──────┐ ┌──────┐  │         │
│  │  - Tray, Menu        │   │  │ SPA  │ │ SPA  │  │         │
│  │  - IPC to Rust       │   │  │(local│ │(remote│  │         │
│  │  - Auth (OIDC)       │   │  │twake:│ │https:)│  │         │
│  │  - Sidecar lifecycle │   │  └──────┘ └──────┘  │         │
│  └──────────┬───────────┘   │                      │         │
│             │ preload.ts    │  contextBridge only   │         │
│             │ contextBridge │  (no Node access)     │         │
│             └───────────────┴──────────────────────┘         │
└───────────────────┬─────────────────────────────────────────┘
                    │ Unix socket / JSON-RPC 2.0
                    ▼
┌─────────────────────────────────────────────────────────────┐
│                  Sync Engine (Rust sidecar)                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    VFS       │  │ Reconcile    │  │   Network    │       │
│  │  (FUSE/      │  │   Engine     │  │   Layer      │       │
│  │   ProjFS)    │  │              │  │              │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

### Security Model

The Electron shell enforces **defense in depth**:

1. **Sandbox ON** (default v20+) -- Renderer processes cannot access OS
2. **Context isolation ON** (default v12+) -- Preload runs in separate world
3. **nodeIntegration OFF** -- Renderers have zero Node.js access
4. **contextBridge** -- Only whitelisted functions exposed to renderers
5. **Custom protocol** (`twake://bundle/`) -- Local SPA served securely, no `file://`
6. **CSP headers** -- Injected via protocol handler
7. **Navigation restriction** -- Only trusted origins allowed

### Performance Strategy

- Create windows with `show: false`, display on `ready-to-show`
- Bundle main process with esbuild (no `node_modules` runtime scan)
- Lazy window creation (open windows on demand)
- Use Electron built-ins (`safeStorage`, `net.fetch`, `Notification`)
- Structured clone for Electron IPC (faster than JSON for buffers)

---

## Detailed Specifications

For implementation details, refer to these design specs:

| Component          | Spec Document                                                                        | Description                                         |
| ------------------ | ------------------------------------------------------------------------------------ | --------------------------------------------------- |
| **Electron Shell** | [electron-shell-design.md](superpowers/specs/electron-shell-design.md)               | Window management, preload, contextBridge, security |
| **VFS Engine**     | [vfs-engine-design.md](superpowers/specs/vfs-engine-design.md)                       | FUSE/ProjFS implementation, placeholder files       |
| **Reconciliation** | [reconciliation-engine-design.md](superpowers/specs/reconciliation-engine-design.md) | Conflict detection, last-write-wins, CRDT migration |
| **IPC Contract**   | [ipc-contract-design.md](superpowers/specs/ipc-contract-design.md)                   | JSON-RPC schema, event types, error codes           |

---

## Technology Stack

### Electron Shell (TypeScript)

| Component         | Technology                                           | Notes                      |
| ----------------- | ---------------------------------------------------- | -------------------------- |
| **Runtime**       | Electron (latest stable)                             | Chromium + Node.js         |
| **Language**      | TypeScript                                           | Strict mode                |
| **Build**         | esbuild (main process), electron-builder (packaging) | Fast bundling              |
| **IPC to Rust**   | JSON-RPC over Unix socket                            | `net` module or raw socket |
| **Token storage** | `safeStorage` API                                    | OS-level encryption        |
| **Local SPA**     | Custom protocol `twake://bundle/`                    | Secure static file serving |

### Sync Engine (Rust)

| Component         | Crate               | Notes                     |
| ----------------- | ------------------- | ------------------------- |
| **Async runtime** | `tokio`             | Multi-threaded            |
| **VFS Linux**     | `fuse3`             | FUSE 3.x bindings         |
| **VFS Windows**   | `projfs`            | ProjFS bindings           |
| **VFS macOS**     | FFI to FileProvider | Native framework          |
| **Database**      | `sqlx`              | Async SQL, SQLite         |
| **IPC server**    | `jsonrpsee`         | JSON-RPC over Unix socket |
| **WebSocket**     | `tokio-tungstenite` | Async WebSocket           |
| **HTTP**          | `reqwest`           | Async HTTP client         |

---

## Development Streams

The project is structured for parallel development across 2 main streams:

| Stream       | Focus                                 | Status                                                |
| ------------ | ------------------------------------- | ----------------------------------------------------- |
| **Stream A** | Electron Shell (TypeScript)           | [STREAM_A_ELECTRON.md](../STREAM_A_ELECTRON.md)       |
| **Stream B** | Sync Core (Rust VFS + reconciliation) | [STREAM_B_SYNC_CORE.md](../STREAM_B_SYNC_CORE.md)     |
| **Stream C** | IPC + Network + Auth (Rust)           | [STREAM_C_IPC_NETWORK.md](../STREAM_C_IPC_NETWORK.md) |

**Interfaces:** See [INTERFACES.md](../INTERFACES.md) for contract definitions.

**Timeline:** See [PLAN.md](../PLAN.md) for development plan.

---

## Critical Design Decisions

See [Architecture Decision Records](adr/) for documented decisions:

- **ADR-0001** -- Project structure and documentation organization
- **ADR-0002** -- Authentication flow (OIDC PKCE)
- **ADR-0003** -- ~~Two-process architecture (CEF + Rust)~~ Superseded
- **ADR-0004** -- Migration to Electron (security, performance, rationale)

---

## Development Phases

### Phase 1: MVP (2-3 weeks)

Electron shell setup, OIDC authentication, local SPA with `window.__twake` bridge.

**Deliverable:** User can authenticate, get a token, and interact with a local SPA via the bridge.

### Phase 2: Sync (2-3 months)

VFS on Linux (FUSE), file sync with placeholders, reconciliation engine.

**Deliverable:** Working prototype with file sync and placeholder support.

### Phase 3: Production Ready (6 months)

Full cross-platform VFS, conflict resolution UI, multiple web apps, auto-update.

**Deliverable:** Production build on all 3 platforms.

### Phase 4: CRDT Migration (6-12 months)

Backend support for Yjs, migrate reconciliation engine to CRDT.

**Deliverable:** Deterministic merge for simultaneous edits.

---

## References

### Documentation

- [Architecture Decision Records](adr/)
- [Development Plan](../PLAN.md)
- [Hackathon Plan](../PLAN_HACKATON.md)
- [Interface Contracts](../INTERFACES.md)

### Implementation Guides

- [Stream A: Electron Shell](../STREAM_A_ELECTRON.md)
- [Stream B: Sync Core](../STREAM_B_SYNC_CORE.md)
- [Stream C: IPC + Network](../STREAM_C_IPC_NETWORK.md)

### External References

- [Electron Documentation](https://www.electronjs.org/docs)
- [Electron Security](https://www.electronjs.org/docs/latest/tutorial/security)
- [FUSE for Linux](https://github.com/libfuse/libfuse)
- [ProjFS for Windows](https://docs.microsoft.com/en-us/windows/win32/fileio/projected-file-system)
- [FileProvider for macOS](https://developer.apple.com/documentation/fileprovider)

---

_Document version: 6.0_
_Last updated: 2026-03-26_
_Status: Final -- Ready for development_
