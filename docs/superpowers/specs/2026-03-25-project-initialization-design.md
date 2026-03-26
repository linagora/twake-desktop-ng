# Project Initialization Design Spec

**Date:** 2026-03-25  
**Component:** Project Setup  
**Stream:** All (A, B, C)  
**Status:** Approved

---

## Overview

This spec defines the approach for initializing the Twake Desktop NG codebase from a documentation-only state to a working codebase that enables parallel development across 3 streams.

**Approach:** Contract-First — Define IPC contract first, then develop in parallel.

---

## Objectives

### Primary Goals

1. **Create codebase structure** — Cargo workspace, CMake build, directory layout
2. **Define IPC contract** — JSON-RPC schema, shared types, event definitions
3. **Enable parallel development** — All 3 streams can work independently after initialization
4. **Validate end-to-end connectivity** — WebView → IPC → VFS → response works

### Scope (Week 1)

**In scope:**
- ✅ Repository structure (cef-shell/, sync-engine/)
- ✅ Build systems (Cargo.toml, CMakeLists.txt)
- ✅ Shared models (FileNode, FileState)
- ✅ IPC contract (methods, events, error codes)
- ✅ IPC server minimal implementation (dummy responses)
- ✅ IPC client skeleton (C++)
- ✅ In-memory VFS implementation
- ✅ End-to-end connectivity test

**Out of scope:**
- ❌ Full VFS implementation (FUSE/ProjFS)
- ❌ Full IPC handlers (delegate to real implementations)
- ❌ CEF window management (minimal for now)
- ❌ Authentication (mock tokens)
- ❌ Network layer (offline only)

---

## Repository Structure

```
twake-desktop-ng/
├── Cargo.toml                    # Rust workspace root
├── CMakeLists.txt                # CEF shell build
├── README.md                     # Setup rapide (5 min)
├── .gitignore
├── docs/
│   ├── spec.md
│   ├── adr/
│   └── superpowers/specs/
├── cef-shell/                    # Stream A - C++
│   ├── CMakeLists.txt
│   └── src/
│       ├── main.cpp
│       ├── app/
│       │   ├── browser_app.cpp/h
│       │   └── render_app.cpp/h
│       ├── browser/
│       │   ├── window_manager.cpp/h
│       │   └── js_bridge.cpp/h
│       └── ipc/
│           └── ipc_client.cpp/h
├── sync-engine/                  # Streams B + C - Rust
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── models/               # Types partagés (Stream B+C)
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
- `models/` est partagé entre B et C (unique source of truth)
- `cef-shell/` est indépendant (C++ seul, IPC client)
- `scripts/` pour les commandes communes

---

## Sequence (J1-J3)

### Day 1 — Structure de base et types communs

**Stream C (lead) :**
```bash
# Créer structure Rust
cargo new sync-engine --lib
cd sync-engine

# Ajouter dépendances
cargo add tokio serde serde_json uuid thiserror async-trait jsonrpsee time tracing tracing-subscriber

# Créer modules
mkdir -p src/models src/ipc src/events
touch src/models/{mod.rs,file_node.rs,file_state.rs}
touch src/ipc/{mod.rs,contract.rs}
touch src/events/{mod.rs,types.rs,bus.rs}
```

**Stream A (parallèle) :**
```bash
# Créer structure C++
mkdir -p cef-shell/src/{app,browser,ipc}
touch cef-shell/CMakeLists.txt

# Télécharger CEF binaries
wget https://cef-builds.spotifycdn.com/cef_binary_122.1.11+g5c8b4c2+chromium-122.0.6261.111_linux64.tar.bz2
tar -xjf cef_binary_*.tar.bz2
mv cef_binary_* cef-shell/cef/
```

**Stream B (parallèle) :**
```bash
# Review les modèles proposés, proposer ajustements
# Préparer VFS trait definition
mkdir -p sync-engine/src/vfs
touch sync-engine/src/vfs/{mod.rs,vfs_trait.rs}
```

**Types partagés (src/models/) :**

```rust
// file_state.rs
use serde::{Deserialize, Serialize};

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
// Source de vérité : INTERFACES.md — 6 variantes (pas de Synced, Hydrated suffit)
```

```rust
// file_node.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use time::OffsetDateTime;

use super::file_state::FileState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub remote_id: Option<String>,  // ID côté serveur Cozy
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,  // ISO 8601
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}
```

**Contract IPC (src/ipc/contract.rs) :**

```rust
use jsonrpsee::core::{RpcResult, SubscriptionResult};
use jsonrpsee::proc_macros::rpc;
use serde::{Deserialize, Serialize};

use crate::models::{FileNode, FileState};

/// File status returned by file.status (simplified for IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,  // ISO 8601 timestamp
}

#[rpc(server, client)]
pub trait TwakeSyncApi {
    #[method(name = "file.status")]
    async fn file_status(&self, path: String) -> RpcResult<FileStatus>;

    #[method(name = "file.hydrate")]
    async fn file_hydrate(&self, path: String) -> RpcResult<bool>;

    #[method(name = "file.list")]
    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> RpcResult<Vec<FileNode>>;

    #[subscription(name = "events.subscribe", item = TwakeEvent)]
    async fn subscribe_events(&self) -> SubscriptionResult;

    #[method(name = "events.emit")]
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()>;
}
```

**Checklist J1 (17:00) :**
- [ ] sync-engine/ créé avec Cargo.toml
- [ ] models/ avec FileNode et FileState
- [ ] contract.rs avec trait TwakeSyncApi
- [ ] `cargo build` passe
- [ ] Point de sync : tous les streams valident les types

---

### Day 2 — IPC Server minimal

**Stream C :**

```rust
// src/ipc/server.rs
use jsonrpsee::server::Server;
use jsonrpsee::core::{RpcResult, SubscriptionResult};
use tracing::{info, error};
use tokio::sync::broadcast;

use crate::ipc::contract::{TwakeSyncApiServer, FileStatus};
use crate::models::{FileNode, FileState};
use crate::events::bus::EventBus;

pub struct SyncEngineApi {
    event_bus: EventBus,
    vfs: Option<Box<dyn VfsBackend>>,  // Will be connected on Day 3
}

#[jsonrpsee::core::async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    async fn file_status(&self, path: String) -> RpcResult<FileStatus> {
        info!("file.status called: {}", path);
        
        // For Day 2: dummy response
        // For Day 3: delegate to VFS
        Ok(FileStatus {
            path,
            state: FileState::Ghost,
            size: 0,
            modified: "2026-03-25T10:00:00Z".to_string(),
        })
    }

    async fn file_hydrate(&self, path: String) -> RpcResult<bool> {
        info!("file.hydrate called: {}", path);
        Ok(true)
    }

    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> RpcResult<Vec<FileNode>> {
        info!("file.list called: {}, recursive: {:?}", path, recursive);
        Ok(vec![])
    }

    async fn subscribe_events(
        &self,
        mut subscription: jsonrpsee::server::SubscriptionSink,
    ) -> SubscriptionResult {
        info!("events.subscribe called");
        
        // Subscribe to event bus and forward events
        let mut rx = self.event_bus.subscribe();
        
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if subscription.send(event).await.is_err() {
                    break;
                }
            }
        });
        
        Ok(())
    }

    async fn emit_event(&self, event: String, data: String) -> RpcResult<()> {
        info!("events.emit called: {} {}", event, data);
        Ok(())
    }
}

pub async fn start_server(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Remove existing socket
    let _ = std::fs::remove_file(socket_path);
    
    let server = Server::builder().build(socket_path).await?;
    let api = SyncEngineApi { 
        event_bus: EventBus::new(),
        vfs: None,
    };
    let _handle = server.start(api.into_rpc());
    info!("IPC server started on {}", socket_path);
    Ok(())
}
```

**Stream A (parallèle) :**

```cpp
// cef-shell/src/ipc/ipc_client.h
class IpcClient {
public:
    bool connect(const std::string& socket_path);
    void disconnect();
    nlohmann::json callMethod(const std::string& method, const nlohmann::json& params);

private:
    int socket_fd_;
    bool connected_;
};
```

**Checklist J2 (17:00) :**
- [ ] IPC server écoute sur `/tmp/twake-ipc.sock`
- [ ] Méthode `file.status` répond avec dummy data
- [ ] IPC client C++ peut se connecter
- [ ] Contrat IPC validé par Stream A

---

### Day 3 — End-to-End Test

**Stream C :**
```rust
// Connecter handlers à InMemoryVfs
pub struct SyncEngineApi {
    vfs: Box<dyn VfsBackend>,  // InMemoryVfs from Stream B
    event_bus: EventBus,
}
```

**Stream A :**
```cpp
// Tester IPC client
IpcClient client;
client.connect("/tmp/twake-ipc.sock");
auto result = client.callMethod("file.status", {{"path", "/test.txt"}});
std::cout << "Result: " << result.dump() << std::endl;
```

**Test E2E :**
```bash
# Terminal 1 : Start IPC server
cd sync-engine
cargo run --bin twake-sync

# Terminal 2 : Test with curl
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'

# Terminal 3 : CEF shell test
cd cef-shell
./build/cef_app --test-ipc
```

**Checklist J3 (17:00) :**
- [ ] `cargo build` passe sans warnings
- [ ] IPC server répond aux requêtes
- [ ] IPC client C++ reçoit réponse
- [ ] E2E test passe avec sortie attendue :
  ```bash
  curl ... file.status /test.txt
  # Expected: {"jsonrpc":"2.0","result":{"path":"/test.txt","state":"ghost",...},"id":1}
  ```
- [ ] Latence E2E < 100ms
- [ ] Contrat IPC validé par tous
- [ ] Go pour développement parallèle J4+

---

## Quality Criteria

### Contract IPC

- Types Rust avec `Serialize + Deserialize`
- Documentation sur chaque méthode (doc comments)
- Exemples de requêtes/réponses dans contract.rs
- Tests unitaires pour sérialisation

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_state_serialization() {
        let json = serde_json::to_string(&FileState::Ghost).unwrap();
        assert_eq!(json, "\"ghost\"");
    }
}
```

### Architecture

- Pas de dépendance circulaire entre modules
- `models/` indépendant (pas de dépendance vers autres modules)
- Build systems isolés (Cargo pour Rust, CMake pour C++)

### Build

- `cargo build` passe sans warnings
- `cmake --build` passe sans warnings
- Scripts de build dans `scripts/`

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **Contrat IPC instable** | J1-J2 dédiés au design, validation par tous à 17:00 |
| **CEF build échoue** | Prébuilt binaries, documenter dans README.md |
| **Incompatibilité types** | Models dans sync-engine/, unique source of truth |
| **IPC disconnects** | Retry logic avec exponential backoff |
| **Stream B trop lent** | InMemoryVfs simple, pas de FUSE cette semaine |

**Plan B :**
- Si IPC trop complexe → fichiers JSON temporaires
- Si CEF bloque → Electron en fallback
- Si FUSE bloque → dossier normal sans placeholders

---

## Dependencies

### External

- CEF 122.x prebuilt binaries
- Rust 1.75+ (tokio, serde, jsonrpsee)
- CMake 3.16+
- FUSE 3.x dev headers (pour FUSE plus tard)

### Internal

- Stream C lead sur J1-J2
- Streams A et B review des types J1
- Point de sync quotidien à 17:00

---

## Success Metrics

**J3 soir :**
- ✅ Codebase structurée (cef-shell/, sync-engine/)
- ✅ Build systems fonctionnels (cargo build, cmake --build)
- ✅ Contrat IPC défini et implémenté
- ✅ E2E test passe (WebView → IPC → VFS → réponse)
- ✅ Tous les streams prêts pour développement parallèle

**KPIs :**
- Temps de build < 5 minutes
- E2E test < 100ms de latence
- 0 warning dans les builds

---

## References

- [PLAN.md](../../PLAN.md) - 6-week development plan
- [INTERFACES.md](../../INTERFACES.md) - Interface contracts
- [ipc-contract-design.md](../ipc-contract-design.md) - Detailed IPC spec
- [STREAM_A_CEF.md](../../STREAM_A_CEF.md) - CEF implementation guide
- [STREAM_B_SYNC_CORE.md](../../STREAM_B_SYNC_CORE.md) - Sync core guide
- [STREAM_C_IPC_NETWORK.md](../../STREAM_C_IPC_NETWORK.md) - IPC/Network guide
