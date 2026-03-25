# Twake Desktop NG - Plan de Développement

**Date:** 2026-03-25  
**Version:** 1.0  
**Équipe:** 3 développeurs + IA  
**Objectif:** Développement parallèle sans blocages mutuels

---

## Architecture du Découpage

```
                    ┌─────────────────────┐
                    │   IPC CONTRACT      │
                    │  (JSON-RPC schema)  │
                    └──────────┬──────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
   ┌─────────────┐     ┌─────────────┐      ┌─────────────┐
   │  Stream A   │     │  Stream B   │      │  Stream C   │
   │   CEF Shell │     │ Sync Core   │      │   IPC +     │
   │   (C++)     │     │  (Rust)     │      │  Network    │
   └─────────────┘     └─────────────┘      └─────────────┘
```

**Principe:** Chacun travaille indépendamment, seules les interfaces sont partagées.

---

## Stream A — CEF Shell (C++)

**Responsable:** Dev 1  
**Stack:** C++, CMake, CEF

### Objectifs

- Shell CEF pour héberger les WebViews Twake
- Gestion des fenêtres natives
- Tray icon et notifications OS
- Bridge JavaScript ↔ IPC
- Client IPC pour communiquer avec le sync engine

### Livrables

#### A1. Infrastructure CEF

- [ ] Setup CMake avec CEF prebuilt binaries
- [ ] CEF initialization (CefInitialize, message loop)
- [ ] Browser creation (multi-window support)
- [ ] Renderer process isolation (one per origin)
- [ ] Crash recovery (renderer crash → reload window)

#### A2. Window Management

- [ ] Create/Close/Minimize/Maximize windows
- [ ] Native window decorations
- [ ] Single instance lock
- [ ] Window persistence (position, size)

#### A3. Native Integration

- [ ] Tray icon (Windows/Mac/Linux)
- [ ] Context menu (open, quit, settings)
- [ ] Native notifications (WinRT/NSUserNotification/libnotify)
- [ ] App launcher (open VFS files with external editors)

#### A4. JavaScript Bridge

- [ ] Bridge injection (`window.__twake`)
- [ ] Domain filtering (only on Twake domains)
- [ ] Method registration (getFileStatus, requestHydration, emit)
- [ ] Event dispatch (Rust → JS via CefProcessMessage)
- [ ] Security: inject only on trusted domains

#### A5. IPC Client

- [ ] JSON-RPC client implementation
- [ ] Unix socket / named pipe connection
- [ ] Method calls to Rust engine
- [ ] Event subscription (receive from Rust)
- [ ] Error handling and retry logic

### Dépendances

**Bloquante:** Contrat IPC (JSON-RPC schema) — 3 jours max d'attente

**Non bloquantes:**

- Peut préparer l'environnement CEF pendant l'attente
- Peut développer le bridge JS sans IPC réel (mock)

### Fichiers Source

```
src/
  cef/
    CMakeLists.txt
    main.cpp
    app/
      browser_app.cpp/h
      render_app.cpp/h
    browser/
      window_manager.cpp/h
      tray_icon.cpp/h
      notification.cpp/h
    bridge/
      js_bridge.cpp/h
      domain_filter.cpp/h
    ipc/
      ipc_client.cpp/h
      json_rpc_client.cpp/h
```

---

## Stream B — Sync Core (Rust)

**Responsable:** Dev 2  
**Stack:** Rust, tokio, FUSE, SQLite

### Objectifs

- Moteur de synchronisation VFS
- Gestion des fichiers placeholders
- Reconciliation avec stratégie last-write-wins
- Base de données locale pour métadonnées

### Livrables

#### B1. Core Models

- [ ] FileNode struct (UUID, path, state, version, parent_id)
- [ ] FileState enum (Ghost, Hydrated, Modified, Syncing, Conflict, Error)
- [ ] Version type (semantic versioning or vector clock)
- [ ] Uuid generation (v4)

#### B2. VFS Trait

- [ ] Common VFS trait definition
- [ ] FileNodeState trait for state management
- [ ] Path resolution (UUID → path, path → UUID)
- [ ] Directory listing (recursive, filtered)
- [ ] File operations (metadata, size, modified time)

#### B3. FUSE Backend (Linux)

- [ ] FUSE 3.x integration (fuse3 crate)
- [ ] File system mounting
- [ ] Placeholder file creation
- [ ] On-demand hydration trigger
- [ ] File event watching (notify crate)

#### B4. Database Layer

- [ ] SQLite schema with sqlx
- [ ] FileNode persistence
- [ ] Version tracking
- [ ] Migrations system
- [ ] Async queries (tokio-postgres or sqlx with tokio)

#### B5. Reconciliation Engine

- [ ] ReconciliationEngine trait definition
- [ ] CouchStyleEngine implementation
- [ ] Last-write-wins strategy
- [ ] Conflict detection (version comparison)
- [ ] Backup copy creation on conflict
- [ ] Manual resolution support

#### B6. Local File Watching

- [ ] File change detection (notify crate)
- [ ] Debouncing for batched changes
- [ ] Modified file tracking
- [ ] Sync trigger on local changes

### Dépendances

**AUCUNE** — 100% indépendant

Peut commencer immédiatement, ne touche pas à l'IPC ni au réseau.

### Fichiers Source

```
sync-engine/
  Cargo.toml
  src/
    lib.rs
    models/
      mod.rs
      file_node.rs
      file_state.rs
      version.rs
      uuid.rs
    vfs/
      mod.rs
      vfs_trait.rs
      fuse_backend.rs
      projfs_backend.rs (placeholder for Windows)
      fileprovider_backend.rs (placeholder for macOS)
    db/
      mod.rs
      schema.rs
      migrations.rs
      repository.rs
    reconciliation/
      mod.rs
      engine_trait.rs
      couch_engine.rs
      conflict_resolver.rs
    watcher/
      mod.rs
      file_watcher.rs
      debouncer.rs
```

### Tests

- [ ] Unit tests for models
- [ ] Integration tests for VFS trait
- [ ] FUSE tests (placeholder creation, hydration)
- [ ] Database tests (CRUD, migrations)
- [ ] Reconciliation tests (conflict detection, resolution)

---

## Stream C — IPC + Network (Rust)

**Responsable:** Dev 3  
**Stack:** Rust, jsonrpsee, tokio, reqwest

### Objectifs

- Définir le contrat IPC (JSON-RPC schema)
- Implémenter le server IPC
- Gérer les événements entre processus
- Authentification OIDC
- Synchronisation réseau avec serveur Twake

### Livrables

#### C1. IPC Contract (PRIORITAIRE — Jour 1-3)

- [ ] JSON-RPC schema definition
- [ ] Method definitions:
  - `file.status(path: String) -> FileStatus`
  - `file.hydrate(path: String) -> Result<()>`
  - `file.list(path: String) -> Vec<FileNode>`
  - `events.subscribe() -> EventStream`
  - `events.emit(event: String, data: String) -> Result<()>`
- [ ] Event type definitions:
  - `FileChanged { path, status }`
  - `SyncStarted { path }`
  - `SyncCompleted { path }`
  - `Conflict { path, versions }`
  - `Notification { app, payload }`
- [ ] Error types and codes
- [ ] Documentation (OpenAPI-style or JSON Schema)

#### C2. IPC Server

- [ ] jsonrpsee server setup (Unix socket)
- [ ] Method handlers (delegate to Sync Core)
- [ ] Event subscription (broadcast channel)
- [ ] Connection management (multiple clients)
- [ ] Graceful shutdown

#### C3. Event Bus

- [ ] tokio::sync::broadcast channel
- [ ] Event publishing (internal + IPC)
- [ ] Event subscription (internal + IPC)
- [ ] Event aggregation (merge events from multiple sources)
- [ ] Event forwarding to CEF shell

#### C4. OIDC Authentication

- [ ] .well-known discovery client
- [ ] PKCE flow implementation
- [ ] Authorization code exchange
- [ ] Token refresh logic
- [ ] Token storage (keyring crate)
- [ ] Session management

#### C5. Network Layer

- [ ] WebSocket client (tokio-tungstenite)
- [ ] SSE client (reqwest)
- [ ] HTTP fallback (reqwest)
- [ ] Connection pooling
- [ ] Retry logic with backoff
- [ ] Offline detection

#### C6. Sync Protocol

- [ ] File metadata sync (remote → local)
- [ ] File upload (local → remote)
- [ ] Delta sync (only changed files)
- [ ] Batch operations
- [ ] Conflict detection (remote vs local)

### Dépendances

**AUCUNE** — Peut commencer immédiatement

Le contrat IPC (C1) est le premier livrable et sert aux Streams A et C.

### Fichiers Source

```
sync-engine/
  src/
    ipc/
      mod.rs
      contract.rs          # JSON-RPC schema
      server.rs            # jsonrpsee server
      types.rs             # Request/Response types
    events/
      mod.rs
      bus.rs               # tokio::broadcast
      types.rs             # Event enum
    auth/
      mod.rs
      oidc.rs              # OIDC PKCE flow
      token_storage.rs     # keyring integration
    network/
      mod.rs
      websocket.rs
      sse.rs
      http_client.rs
    sync/
      mod.rs
      protocol.rs          # Sync protocol with server
      uploader.rs
      downloader.rs
```

### Tests

- [ ] IPC contract tests (schema validation)
- [ ] IPC server tests (method calls, subscriptions)
- [ ] Event bus tests (publish/subscribe)
- [ ] OIDC tests (mock SSO server)
- [ ] Network tests (mock server)

---

## Ordre d'Exécution

### Semaine 1

```
Jour 1-3:
├─► Dev C: Écrire contrat IPC (C1) — PRIORITAIRE
├─► Dev B: Commencer Sync Core (B1, B2, B4)
└─► Dev A: Préparer CEF build environment (CEF binaries, CMake)

Jour 4-5:
├─► Dev C: IPC Server (C2) + Event Bus (C3)
├─► Dev B: FUSE Backend (B3) + Reconciliation (B5)
└─► Dev A: Commencer CEF Shell (A1, A2) + IPC Client (A5)
```

### Semaine 2-4

```
Stream A (CEF):
├─ Semaine 2: Window management, tray, notifications (A2, A3)
├─ Semaine 3: JavaScript bridge (A4)
└─ Semaine 4: Integration tests, polish

Stream B (Sync Core):
├─ Semaine 2: FUSE backend (B3) + Database (B4)
├─ Semaine 3: Reconciliation engine (B5) + Watcher (B6)
└─ Semaine 4: Integration tests, polish

Stream C (IPC + Network):
├─ Semaine 2: Event bus (C3) + OIDC (C4)
├─ Semaine 3: Network layer (C5) + Sync protocol (C6)
└─ Semaine 4: Integration tests, polish
```

### Semaine 5-6: Integration

```
├─► Connecter Stream A ↔ Stream C (IPC)
├─► Connecter Stream B ↔ Stream C (Event bus)
├─► End-to-end tests
├─► Performance tuning
└─► Bug fixes
```

---

## Interfaces de Contrat

### IPC Contract (JSON-RPC)

**Transport:** Unix socket (Linux/Mac) / Named pipe (Windows)

**Schema:**

```json
{
  "methods": [
    {
      "name": "file.status",
      "params": { "path": "string" },
      "returns": "FileStatus"
    },
    {
      "name": "file.hydrate",
      "params": { "path": "string" },
      "returns": "Result<void, Error>"
    },
    {
      "name": "file.list",
      "params": { "path": "string", "recursive": "boolean" },
      "returns": "Vec<FileNode>"
    },
    {
      "name": "events.subscribe",
      "params": { "events": "string[]" },
      "returns": "Subscription"
    },
    {
      "name": "events.emit",
      "params": { "event": "string", "data": "string" },
      "returns": "Result<void, Error>"
    }
  ],
  "events": [
    {
      "name": "file.changed",
      "payload": { "path": "string", "status": "FileState" }
    },
    {
      "name": "sync.started",
      "payload": { "path": "string" }
    },
    {
      "name": "sync.completed",
      "payload": { "path": "string", "duration": "number" }
    },
    {
      "name": "conflict.detected",
      "payload": { "path": "string", "versions": "Version[]" }
    }
  ]
}
```

### CEF Bridge API (JavaScript)

```javascript
window.__twake = {
  // Synchronous methods
  getFileStatus(path: string): FileStatus,
  hydrateFile(path: string): Promise<void>,
  listFiles(path: string, recursive: boolean): FileNode[],

  // Event subscription
  subscribe(event: string): void,
  unsubscribe(event: string): void,

  // Event emission
  emit(event: string, data: object): void
}
```

### VFS Trait (Rust)

```rust
pub trait VfsBackend: Send + Sync {
    fn mount(&self, path: &Path) -> Result<()>;
    fn unmount(&self) -> Result<()>;
    fn get_node(&self, path: &Path) -> Result<FileNode>;
    fn create_placeholder(&self, path: &Path, metadata: FileMetadata) -> Result<()>;
    fn hydrate(&self, path: &Path) -> Result<()>;
    fn watch(&self) -> Result<WatchStream>;
}
```

---

## Points de Synchronisation

### Points de contrôle obligatoires

1. **Jour 3:** Contrat IPC validé par les 3 développeurs
2. **Semaine 2:** Stream A peut appeler Stream C via IPC (test de bout en bout)
3. **Semaine 3:** Stream B peut émettre des événements vers Stream C
4. **Semaine 5:** Integration complète, tests E2E

### Communication

- **Daily sync:** 15 min pour aligner sur les interfaces
- **Interface changes:** Discord + mise à jour du contrat IPC
- **Blockers:** Signaler immédiatement, pas d'attente > 1 jour

---

## Risques et Mitigations

| Risque                | Impact | Mitigation                                        |
| --------------------- | ------ | ------------------------------------------------- |
| CEF build complexe    | High   | Utiliser prébuilt binaries, documenter setup      |
| IPC contract instable | Medium | Versionner le contrat, backward compatible        |
| VFS crash (FUSE)      | High   | Isoler dans processus séparé, restart automatique |
| Conflict resolution   | Medium | Last-write-wins + backup (Phase 1), user choice   |
| OIDC SSO complexity   | Medium | Mock SSO pour dev, vrai SSO pour prod             |

---

## Checklist Globale

### Semaine 1

- [ ] Contrat IPC écrit et validé (Dev C)
- [ ] Environnement CEF prêt (Dev A)
- [ ] Sync Core models + trait (Dev B)

### Semaine 2-4

- [ ] CEF Shell fonctionnel (Dev A)
- [ ] Sync Core VFS fonctionnel (Dev B)
- [ ] IPC Server + Network fonctionnel (Dev C)

### Semaine 5-6

- [ ] Integration complète
- [ ] Tests E2E
- [ ] Performance tuning
- [ ] MVP prêt

---

## Notes

- **Pas d'attente:** Chaque stream peut avancer indépendamment
- **Interfaces stables:** Une fois le contrat IPC écrit, ne pas le changer sans accord
- **Tests:** Chaque stream doit avoir ses tests unitaires
- **CI/CD:** Mettre en place après Semaine 2 (quand les streams sont stables)
