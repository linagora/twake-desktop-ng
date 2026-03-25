# Twake Desktop NG - Technical Specification

**Version:** 5.0  
**Date:** 2026-03-25  
**Status:** Final

---

## Executive Summary

Twake Desktop NG is a **collaborative work platform** (not just a file sync client) that provides an enriched desktop experience for Twake web services. The architecture combines a **native C++ shell with CEF** for web rendering with a **Rust sync engine** for VFS, reconciliation, and network operations.

**Key differentiator:** Multi-actor collaboration with offline support, where users can edit the same document via different interfaces (local VFS editor vs. OnlyOffice web) with automatic reconciliation.

**Architecture:** Two-process model with CEF shell (C++) and sync engine (Rust) communicating via IPC. See [ADR-0003](../adr/ADR-0003-two-process-architecture.md) for rationale.

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
3. ✅ Open a web app in a native window (online)

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
└───────────────────┬─────────────────────────────────────────┘
                    │ Unix socket / JSON-RPC
                    ▼
┌─────────────────────────────────────────────────────────────┐
│                  Sync Engine (Rust)                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    VFS       │  │ Reconcile    │  │   Network    │       │
│  │  (FUSE/      │  │   Engine     │  │   Layer      │       │
│  │   ProjFS)    │  │              │  │              │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

---

## Detailed Specifications

For implementation details, refer to these design specs:

| Component | Spec Document | Description |
|-----------|---------------|-------------|
| **CEF Shell** | [cef-shell-design.md](../superpowers/specs/cef-shell-design.md) | Window management, JS bridge, IPC client |
| **VFS Engine** | [vfs-engine-design.md](../superpowers/specs/vfs-engine-design.md) | FUSE/ProjFS implementation, placeholder files |
| **Reconciliation** | [reconciliation-engine-design.md](../superpowers/specs/reconciliation-engine-design.md) | Conflict detection, last-write-wins, CRDT migration |
| **IPC Contract** | [ipc-contract-design.md](../superpowers/specs/ipc-contract-design.md) | JSON-RPC schema, event types, error codes |

---

## Technology Stack

### CEF Shell (C++)

| Component | Technology | Notes |
|-----------|------------|-------|
| **CEF** | Chromium Embedded Framework | Latest stable branch |
| **Build** | CMake + CEF binaries | Prebuilt from Spotify CDN |
| **Window management** | Native OS APIs | Win32 / Cocoa / GTK |
| **IPC client** | JSON-RPC over Unix socket | `cppipc` or custom |

### Sync Engine (Rust)

| Component | Crate | Notes |
|-----------|-------|-------|
| **Async runtime** | `tokio` | Multi-threaded |
| **VFS Linux** | `fuse3` | FUSE 3.x bindings |
| **VFS Windows** | `projfs` | ProjFS bindings |
| **VFS macOS** | FFI to FileProvider | Native framework |
| **Database** | `sqlx` | Async SQL, SQLite |
| **IPC server** | `jsonrpsee` | JSON-RPC over Unix socket |
| **WebSocket** | `tokio-tungstenite` | Async WebSocket |
| **HTTP** | `reqwest` | Async HTTP client |

---

## Development Streams

The project is structured for parallel development across 3 streams:

| Stream | Focus | Lead | Status |
|--------|-------|------|--------|
| **Stream A** | CEF Shell (C++) | Dev 1 | [STREAM_A_CEF.md](../../STREAM_A_CEF.md) |
| **Stream B** | Sync Core (Rust VFS) | Dev 2 | [STREAM_B_SYNC_CORE.md](../../STREAM_B_SYNC_CORE.md) |
| **Stream C** | IPC + Network (Rust) | Dev 3 | [STREAM_C_IPC_NETWORK.md](../../STREAM_C_IPC_NETWORK.md) |

**Interfaces:** See [INTERFACES.md](../../INTERFACES.md) for contract definitions.

**Timeline:** See [PLAN.md](../../PLAN.md) for 6-week development plan.

---

## Critical Design Decisions

See [Architecture Decision Records](../adr/) for documented decisions:

- **ADR-0001** - Project structure and documentation organization
- **ADR-0002** - Authentication flow (OIDC PKCE)
- **ADR-0003** - Two-process architecture (CEF + Rust)

---

## Development Phases

### Phase 1: POC (2-3 weeks)

CEF shell setup, IPC contract, basic window management, bridge JS ↔ C++.

**Deliverable:** Working POC on Linux, WebView can call Rust engine.

### Phase 2: MVP (3 months)

OIDC authentication, VFS on Linux, CouchDB-style reconciliation, one web app.

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

- [Architecture Decision Records](../adr/)
- [Development Plan](../../PLAN.md)
- [Hackathon Plan](../../PLAN_HACKATON.md)
- [Interface Contracts](../../INTERFACES.md)

### Implementation Guides

- [Stream A: CEF Shell](../../STREAM_A_CEF.md)
- [Stream B: Sync Core](../../STREAM_B_SYNC_CORE.md)
- [Stream C: IPC + Network](../../STREAM_C_IPC_NETWORK.md)

### External References

- [CEF Documentation](https://bitbucket.org/chromiumembedded/cef)
- [FUSE for Linux](https://github.com/libfuse/libfuse)
- [ProjFS for Windows](https://docs.microsoft.com/en-us/windows/win32/fileio/projected-file-system)
- [FileProvider for macOS](https://developer.apple.com/documentation/fileprovider)

---

_Document version: 5.0_  
_Last updated: 2026-03-25_  
_Status: Final — Ready for development_
