# Project Initialization Design Spec

**Date:** 2026-03-26
**Component:** Project Setup
**Stream:** All (A, B, C)
**Status:** Approved

---

## Overview

This spec defines the approach for initializing the Twake Desktop NG codebase from a documentation-only state to a working codebase that enables parallel development across 3 streams.

**Approach:** Contract-First -- Define IPC contract first, then develop in parallel.

---

## Objectives

### Primary Goals

1. **Create codebase structure** -- npm project (Electron), Cargo workspace (Rust), directory layout
2. **Define IPC contract** -- JSON-RPC schema, shared types, event definitions
3. **Enable parallel development** -- All 3 streams can work independently after initialization
4. **Validate end-to-end connectivity** -- SPA → Bridge → Main → Sidecar → Rust → response works

### Scope (Week 1)

**In scope:**

- Repository structure (electron-shell/, sync-engine/)
- Build systems (package.json + esbuild, Cargo.toml)
- Shared models (FileNode, FileState)
- IPC contract (methods, events, error codes)
- IPC server minimal implementation (dummy responses)
- Electron shell with contextBridge + mock IPC
- Custom protocol handler (`twake://bundle/`)
- In-memory VFS implementation
- End-to-end connectivity test

**Out of scope:**

- Full VFS implementation (FUSE/ProjFS)
- Full IPC handlers (delegate to real implementations)
- Authentication (mock tokens)
- Network layer (offline only)

---

## Repository Structure

```
twake-desktop-ng/
├── README.md                     # Setup rapide (5 min)
├── .gitignore
├── docs/
│   ├── spec.md
│   ├── adr/
│   └── superpowers/specs/
├── electron-shell/               # Stream A - TypeScript
│   ├── package.json
│   ├── tsconfig.json
│   ├── electron-builder.yml
│   └── src/
│       ├── main.ts
│       ├── preload.ts
│       ├── windows.ts
│       ├── protocol.ts
│       ├── ipc-bridge.ts
│       ├── sidecar.ts
│       ├── auth.ts
│       └── tray.ts
│   └── renderer/
│       ├── index.html
│       ├── app.js
│       └── styles.css
├── sync-engine/                  # Streams B + C - Rust
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── models/               # Types partages (Stream B+C)
│       │   ├── mod.rs
│       │   ├── file_node.rs
│       │   └── file_state.rs
│       ├── vfs/                  # Stream B
│       │   ├── mod.rs
│       │   ├── vfs_trait.rs
│       │   └── in_memory_vfs.rs
│       ├── reconciliation/       # Stream B
│       │   ├── mod.rs
│       │   └── engine_trait.rs
│       ├── ipc/                  # Stream C
│       │   ├── mod.rs
│       │   ├── contract.rs
│       │   ├── server.rs
│       │   └── handlers.rs
│       ├── events/               # Stream C
│       │   ├── mod.rs
│       │   ├── bus.rs
│       │   └── types.rs
│       ├── auth/                 # Stream C
│       │   ├── mod.rs
│       │   └── oidc.rs
│       └── network/              # Stream C
│           ├── mod.rs
│           └── client.rs
└── scripts/
    ├── build.sh
    ├── test.sh
    └── setup.sh
```

**Key principles:**

- `sync-engine/` contient tout le code Rust (Streams B et C)
- `models/` est partage entre B et C (unique source of truth)
- `electron-shell/` est independant (TypeScript seul, IPC via Unix socket)
- `scripts/` pour les commandes communes

---

## Sequence (J1-J3)

### Day 1 -- Structure de base et types communs

**Stream C (lead) :**

```bash
# Creer structure Rust
cargo new sync-engine --lib
cd sync-engine
cargo add tokio serde serde_json uuid thiserror async-trait jsonrpsee time tracing tracing-subscriber
mkdir -p src/models src/ipc src/events
```

**Stream A (parallele) :**

```bash
# Creer structure Electron
mkdir -p electron-shell/src electron-shell/renderer
cd electron-shell
npm init -y
npm install --save-dev electron typescript esbuild @types/node
npx tsc --init --strict --target es2022 --module commonjs --outDir dist
```

**Stream B (parallele) :**

```bash
# Review les modeles proposes, preparer VFS trait
mkdir -p sync-engine/src/vfs
```

**Types partages (src/models/) :**

```rust
// file_state.rs
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
// Source de verite : INTERFACES.md -- 6 variantes
```

```rust
// file_node.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub remote_id: Option<String>,
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,  // ISO 8601
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}
```

**Checklist J1 (17:00) :**

- [ ] sync-engine/ cree avec Cargo.toml
- [ ] electron-shell/ cree avec package.json
- [ ] models/ avec FileNode et FileState
- [ ] contract.rs avec trait TwakeSyncApi
- [ ] `cargo build` passe
- [ ] `npm run build` passe
- [ ] Point de sync : tous les streams valident les types

---

### Day 2 -- IPC Server + Electron shell minimal

**Stream C :**

```rust
// IPC server minimal avec dummy responses
pub async fn start_server(socket_path: &str) -> Result<()> { ... }
```

**Stream A :**

```typescript
// Electron shell with:
// - BrowserWindow (sandbox ON, contextIsolation ON)
// - preload.ts with contextBridge
// - protocol handler twake://bundle/
// - IPC mock handlers
// - SPA renderer/index.html
```

**Checklist J2 (17:00) :**

- [ ] IPC server ecoute sur Unix socket
- [ ] Methode `file.status` repond avec dummy data
- [ ] Electron shell ouvre fenetre avec SPA locale
- [ ] contextBridge injecte `window.__twake`
- [ ] SPA peut appeler bridge (mock)

---

### Day 3 -- End-to-End Test

**Test E2E :**

```bash
# Terminal 1 : Start Rust IPC server
cd sync-engine && cargo run --bin twake-sync

# Terminal 2 : Test with curl
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'

# Terminal 3 : Electron shell
cd electron-shell && npm start
# → SPA loads, calls window.__twake.getFileStatus("/test.txt")
```

**Checklist J3 (17:00) :**

- [ ] `cargo build` passe sans warnings
- [ ] `npm run build` passe sans warnings
- [ ] IPC server repond aux requetes (curl test)
- [ ] Electron SPA charge via `twake://bundle/index.html`
- [ ] Bridge appelle mock handlers
- [ ] Latence E2E < 100ms
- [ ] Contrat IPC valide par tous
- [ ] Go pour developpement parallele J4+

---

## Quality Criteria

### Contract IPC

- Types Rust avec `Serialize + Deserialize`
- Documentation sur chaque methode (doc comments)
- Exemples de requetes/reponses dans contract.rs
- Tests unitaires pour serialisation

### Architecture

- Pas de dependance circulaire entre modules
- `models/` independant (pas de dependance vers autres modules)
- Build systems isoles (npm pour Electron, Cargo pour Rust)

### Build

- `cargo build` passe sans warnings
- `npm run build` passe sans warnings
- Scripts de build dans `scripts/`

---

## Risk Mitigation

| Risk                      | Mitigation                                               |
| ------------------------- | -------------------------------------------------------- |
| **Contrat IPC instable**  | J1-J2 dedies au design, validation par tous a 17:00      |
| **Electron build echoue** | npm install est fiable, electron-quick-start en fallback |
| **Incompatibilite types** | Models dans sync-engine/, unique source of truth         |
| **IPC disconnects**       | Retry logic avec exponential backoff                     |
| **Stream B trop lent**    | InMemoryVfs simple, pas de FUSE cette semaine            |

**Plan B :**

- Si IPC trop complexe → fichiers JSON temporaires
- Si FUSE bloque → dossier normal sans placeholders

---

## Dependencies

### External

- Node.js 20+ (Electron)
- Rust 1.75+ (tokio, serde, jsonrpsee)
- FUSE 3.x dev headers (pour FUSE plus tard)

### Internal

- Stream C lead sur J1-J2
- Streams A et B review des types J1
- Point de sync quotidien a 17:00

---

## Success Metrics

**J3 soir :**

- Codebase structuree (electron-shell/, sync-engine/)
- Build systems fonctionnels (npm run build, cargo build)
- Contrat IPC defini et implemente
- SPA charge dans Electron avec bridge fonctionnel
- Tous les streams prets pour developpement parallele

**KPIs :**

- Temps de build < 30 secondes (Electron), < 5 minutes (Rust)
- E2E test < 100ms de latence
- 0 warning dans les builds

---

## References

- [PLAN.md](../../../PLAN.md) - Development plan
- [INTERFACES.md](../../../INTERFACES.md) - Interface contracts
- [ipc-contract-design.md](ipc-contract-design.md) - Detailed IPC spec
- [electron-shell-design.md](electron-shell-design.md) - Electron shell spec
- [STREAM_A_ELECTRON.md](../../../STREAM_A_ELECTRON.md) - Electron implementation guide
- [STREAM_B_SYNC_CORE.md](../../../STREAM_B_SYNC_CORE.md) - Sync core guide
- [STREAM_C_IPC_NETWORK.md](../../../STREAM_C_IPC_NETWORK.md) - IPC/Network guide
