# Stream C — IPC + Network (Rust)

**Responsable:** Dev 3  
**Stack:** Rust, jsonrpsee, tokio, reqwest  
**Objectif:** IPC server, event bus, OIDC auth, network layer

---

## Jour 1 — Contrat IPC et Server

### Matin (08:00 - 10:00) — Setup Cargo

**Tâche C1.1: Ajouter dépendances au projet**
```bash
# Dans sync-engine/Cargo.toml (déjà créé par Stream B)
# Ajouter:

[dependencies]
# IPC
jsonrpsee = { version = "0.22", features = ["server", "macros"] }

# Network
reqwest = { version = "0.11", features = ["json"] }
tokio-tungstenite = "0.21"

# Crypto
base64 = "0.21"
sha2 = "0.10"
rand = "0.8"

# Time
time = { version = "0.3", features = ["serde"] }

# Async trait
async-trait = "0.1"
```

**Tâche C1.2: Module structure**
```
sync-engine/
├── src/
│   ├── ipc/
│   │   ├── mod.rs
│   │   ├── contract.rs      # JSON-RPC trait definition
│   │   ├── server.rs        # jsonrpsee server
│   │   └── handlers.rs      # Method implementations
│   ├── events/
│   │   ├── mod.rs
│   │   ├── bus.rs           # Event bus
│   │   └── types.rs         # Event definitions
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── oidc.rs          # OIDC PKCE flow
│   │   └── token_storage.rs
│   └── network/
│       ├── mod.rs
│       ├── client.rs        # HTTP client
│       └── websocket.rs     # WebSocket client
```

**Critère de succès:** `cargo build` passe

---

### Matin (10:00 - 12:00) — Contrat IPC (PRIORITAIRE)

**Tâche C1.3: Définition des types partagés**
```rust
// src/ipc/contract.rs
use serde::{Deserialize, Serialize};
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;

use crate::models::{FileNode, FileState};

/// JSON-RPC contract for IPC communication
#[rpc(server, client)]
pub trait TwakeSyncApi {
    /// Get file status (state, size, modified time)
    #[method(name = "file.status")]
    async fn file_status(&self, path: String) -> RpcResult<FileStatus>;
    
    /// Hydrate a ghost file (download content)
    #[method(name = "file.hydrate")]
    async fn file_hydrate(&self, path: String) -> RpcResult<bool>;
    
    /// List directory contents
    #[method(name = "file.list")]
    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> RpcResult<Vec<FileNode>>;
    
    /// Subscribe to events
    #[subscription(name = "events.subscribe", item = TwakeEvent)]
    async fn subscribe_events(&self) -> SubscriptionResult;
    
    /// Emit an event (from WebView)
    #[method(name = "events.emit")]
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()>;
}

/// File status (simplified for IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,  // ISO 8601
}

/// Events that can be published/subscribed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TwakeEvent {
    FileChanged {
        path: String,
        state: FileState,
    },
    SyncStarted {
        path: String,
    },
    SyncCompleted {
        path: String,
        duration_ms: u64,
    },
    Error {
        code: i32,
        message: String,
    },
}
```

**Tâche C1.4: Exemple de requête/réponse**
```rust
// src/ipc/examples.rs
/// Example: How to call the IPC server from C++
///
/// Request:
/// {
///   "jsonrpc": "2.0",
///   "method": "file.status",
///   "params": { "path": "/documents/test.txt" },
///   "id": 1
/// }
///
/// Response:
/// {
///   "jsonrpc": "2.0",
///   "result": {
///     "path": "/documents/test.txt",
///     "state": "ghost",
///     "size": 1024,
///     "modified": "2026-03-25T10:00:00Z"
///   },
///   "id": 1
/// }

/// Example: Event subscription
///
/// Request:
/// {
///   "jsonrpc": "2.0",
///   "method": "events.subscribe",
///   "params": [],
///   "id": 2
/// }
///
/// Response (subscription ID):
/// {
///   "jsonrpc": "2.0",
///   "result": { "subscription": 1 },
///   "id": 2
/// }
///
/// Events (push):
/// {
///   "jsonrpc": "2.0",
///   "method": "events.subscribe",
///   "params": {
///     "subscription": 1,
///     "result": {
///       "type": "file_changed",
///       "path": "/documents/test.txt",
///       "state": "hydrated"
///     }
///   }
/// }
```

**Tâche C1.5: Validation du contrat**
```rust
// src/ipc/tests.rs
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_status_serialization() {
        let status = FileStatus {
            path: "/test.txt".to_string(),
            state: FileState::Ghost,
            size: 1024,
            modified: "2026-03-25T10:00:00Z".to_string(),
        };
        
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("ghost"));
    }
    
    #[test]
    fn test_event_serialization() {
        let event = TwakeEvent::FileChanged {
            path: "/test.txt".to_string(),
            state: FileState::Hydrated,
        };
        
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("file_changed"));
    }
}
```

**Critère de succès:** Contrat IPC validé par tests

---

### Après-midi (14:00 - 16:00) — IPC Server

**Tâche C1.6: Server setup**
```rust
// src/ipc/server.rs
use std::net::SocketAddr;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::types::ErrorObject;
use tokio::sync::broadcast;
use tracing::info;

use super::contract::{TwakeSyncApiServer, TwakeSyncApiServerImpl, TwakeEvent, FileStatus};
use crate::models::FileNode;

pub struct SyncEngineApi {
    // References to actual implementation
    // Will be injected from main
}

#[jsonrpsee::core::async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    async fn file_status(&self, path: String) -> RpcResult<FileStatus> {
        // Delegate to Stream B's VFS
        Ok(FileStatus {
            path,
            state: crate::models::FileState::Ghost,
            size: 0,
            modified: "2026-03-25T10:00:00Z".to_string(),
        })
    }
    
    async fn file_hydrate(&self, path: String) -> RpcResult<bool> {
        // Delegate to Stream B's hydration service
        Ok(true)
    }
    
    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> RpcResult<Vec<FileNode>> {
        // Delegate to Stream B's VFS
        Ok(vec![])
    }
    
    async fn subscribe_events(
        &self,
        subscription: jsonrpsee::server::SubscriptionSink,
    ) -> SubscriptionResult {
        // Subscribe to event bus
        Ok(())
    }
    
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()> {
        // Forward to event bus
        Ok(())
    }
}

pub async fn start_server(socket_path: &str) -> Result<ServerHandle, Box<dyn std::error::Error>> {
    info!("Starting IPC server on {}", socket_path);
    
    let server = Server::builder()
        .build(socket_path)
        .await?;
    
    let api = SyncEngineApi {};
    let handle = server.start(api.into_rpc());
    
    info!("IPC server started");
    Ok(handle)
}
```

**Tâche C1.7: Unix socket server**
```rust
// src/ipc/server.rs (continued)
use jsonrpsee::server::ServerBuilder;
use jsonrpsee::server::unix::UnixServerBuilder;

pub async fn start_unix_server(
    socket_path: &str,
) -> Result<ServerHandle, Box<dyn std::error::Error>> {
    // Remove existing socket
    let _ = std::fs::remove_file(socket_path);
    
    let server = Server::builder()
        .build(socket_path)
        .await?;
    
    let api = SyncEngineApi {};
    let handle = server.start(api.into_rpc());
    
    Ok(handle)
}
```

**Critère de succès:** IPC server écoute sur Unix socket

---

### Après-midi (16:00 - 18:00) — Event Bus

**Tâche C1.8: Event bus implementation**
```rust
// src/events/bus.rs
use tokio::sync::broadcast;
use tracing::info;
use serde_json::Value;

use super::types::TwakeEvent;

const EVENT_BUFFER_SIZE: usize = 256;

pub struct EventBus {
    tx: broadcast::Sender<TwakeEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(EVENT_BUFFER_SIZE);
        Self { tx }
    }
    
    pub async fn publish(&self, event: TwakeEvent) -> Result<(), broadcast::error::SendError<TwakeEvent>> {
        info!("Event published: {:?}", event);
        self.tx.send(event)?;
        Ok(())
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<TwakeEvent> {
        self.tx.subscribe()
    }
    
    pub async fn publish_file_changed(&self, path: String, state: crate::models::FileState) {
        let event = TwakeEvent::FileChanged { path, state };
        self.publish(event).await.ok();
    }
    
    pub async fn publish_sync_started(&self, path: String) {
        let event = TwakeEvent::SyncStarted { path };
        self.publish(event).await.ok();
    }
    
    pub async fn publish_sync_completed(&self, path: String, duration_ms: u64) {
        let event = TwakeEvent::SyncCompleted { path, duration_ms };
        self.publish(event).await.ok();
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
```

**Tâche C1.9: Event types**
```rust
// src/events/types.rs
use serde::{Deserialize, Serialize};
use crate::models::FileState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TwakeEvent {
    FileChanged {
        path: String,
        state: FileState,
    },
    SyncStarted {
        path: String,
    },
    SyncCompleted {
        path: String,
        duration_ms: u64,
    },
    Error {
        code: i32,
        message: String,
    },
}
```

**Tâche C1.10: Event loop**
```rust
// src/events/mod.rs
pub mod bus;
pub mod types;

use tokio::sync::broadcast;
use tracing::info;

use super::types::TwakeEvent;

pub async fn event_loop(
    mut rx: broadcast::Receiver<TwakeEvent>,
) {
    while let Ok(event) = rx.recv().await {
        info!("Event received: {:?}", event);
        // Forward to CEF shell via IPC
        // Handle internally (notifications, sync triggers, etc.)
    }
}
```

**Critère de succès:** Event bus publish/subscribe fonctionne

---

## Jour 2 — Auth et Network

### Matin (08:00 - 10:00) — OIDC PKCE

**Tâche C2.1: PKCE helpers**
```rust
// src/auth/oidc.rs
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

pub fn generate_pkce_codes() -> PkceCodes {
    // Generate random code verifier
    let mut verifier = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier);
    
    let code_verifier = URL_SAFE_NO_PAD.encode(&verifier);
    
    // Generate code challenge (SHA256 of verifier)
    let mut hasher = Sha256::new();
    hasher.update(&verifier);
    let challenge = hasher.finalize();
    
    let code_challenge = URL_SAFE_NO_PAD.encode(&challenge);
    
    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pkce_generation() {
        let codes = generate_pkce_codes();
        assert_eq!(codes.code_verifier.len(), 43);  // Base64 of 32 bytes
        assert_eq!(codes.code_challenge.len(), 43);
    }
}
```

**Tâche C2.2: Token storage**
```rust
// src/auth/token_storage.rs
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use chrono::Utc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

pub struct TokenStorage {
    path: PathBuf,
}

impl TokenStorage {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let path = PathBuf::from(home).join(".twake").join("tokens.json");
        
        // Create directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        
        Self { path }
    }
    
    pub fn save(&self, token: &TokenResponse) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(token)?;
        fs::write(&self.path, json)?;
        info!("Token saved to {:?}", self.path);
        Ok(())
    }
    
    pub fn load(&self) -> Option<TokenResponse> {
        let json = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&json).ok()
    }
    
    pub fn is_valid(&self) -> bool {
        if let Some(token) = self.load() {
            let expires_at = Utc::now().timestamp() as u64 + token.expires_in;
            expires_at > Utc::now().timestamp() as u64
        } else {
            false
        }
    }
}
```

**Critère de succès:** PKCE codes générés, token storage fonctionne

---

### Matin (10:00 - 12:00) — HTTP Client

**Tâche C2.3: Network client**
```rust
// src/network/client.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::auth::token_storage::{TokenResponse, TokenStorage};

pub struct TwakeClient {
    http: Client,
    base_url: String,
    token_storage: TokenStorage,
}

impl TwakeClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.to_string(),
            token_storage: TokenStorage::new(),
        }
    }
    
    pub async fn get_metadata(&self, path: &str) -> Result<serde_json::Value, reqwest::Error> {
        let url = format!("{}/api/v1/files{}", self.base_url, path);
        
        let token = self.token_storage.load();
        let mut req = self.http.get(&url);
        
        if let Some(ref t) = token {
            req = req.bearer_auth(&t.access_token);
        }
        
        let response = req.send().await?;
        response.json().await
    }
    
    pub async fn download_file(&self, path: &str, dest: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/api/v1/files{}/content", self.base_url, path);
        
        let token = self.token_storage.load();
        let mut req = self.http.get(&url);
        
        if let Some(ref t) = token {
            req = req.bearer_auth(&t.access_token);
        }
        
        let response = req.send().await?;
        let bytes = response.bytes().await?;
        
        tokio::fs::write(dest, &bytes).await?;
        info!("Downloaded {} bytes to {:?}", bytes.len(), dest);
        
        Ok(())
    }
}
```

**Tâche C2.4: Mock server (for testing)**
```rust
// src/network/mock.rs
#[cfg(test)]
pub mod mock_server {
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};
    
    pub async fn start_mock_server() -> MockServer {
        let mock_server = MockServer::start().await;
        
        Mock::given(method("GET"))
            .and(path("/api/v1/files/test.txt"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_string(r#"{"id": "123", "name": "test.txt", "size": 1024}"#))
            .mount(&mock_server)
            .await;
        
        mock_server
    }
}
```

**Critère de succès:** HTTP client peut appeler API Twake

---

### Après-midi (14:00 - 16:00) — Integration Handlers

**Tâche C2.5: IPC handlers implementation**
```rust
// src/ipc/handlers.rs
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::types::ErrorObject;
use tracing::info;

use super::contract::{TwakeSyncApiServer, TwakeEvent, FileStatus};
use crate::models::{FileNode, FileState};
use crate::events::bus::EventBus;
use crate::vfs::vfs_trait::VfsBackend;
use crate::services::hydration::HydrationService;

pub struct SyncEngineApi {
    vfs: Box<dyn VfsBackend>,
    hydration: HydrationService,
    event_bus: EventBus,
}

#[jsonrpsee::core::async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    async fn file_status(&self, path: String) -> Result<FileStatus, ErrorObject<'_>> {
        info!("file.status called: {}", path);
        
        let node = self.vfs.get_node(std::path::Path::new(&path))
            .await
            .map_err(|e| ErrorObject::owned(-32000, e.to_string(), None::<()>))?;
        
        Ok(FileStatus {
            path: node.path,
            state: node.state,
            size: node.size,
            modified: node.modified.to_rfc3339(),
        })
    }
    
    async fn file_hydrate(&self, path: String) -> Result<bool, ErrorObject<'_>> {
        info!("file.hydrate called: {}", path);
        
        self.event_bus.publish_sync_started(path.clone()).await;
        
        self.hydration.hydrate_file(std::path::Path::new(&path))
            .await
            .map_err(|e| ErrorObject::owned(-32000, e.to_string(), None::<()>))?;
        
        self.event_bus.publish_file_changed(path, FileState::Hydrated).await;
        
        Ok(true)
    }
    
    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> Result<Vec<FileNode>, ErrorObject<'_>> {
        info!("file.list called: {}, recursive: {:?}", path, recursive);
        
        let nodes = self.vfs.list_dir(std::path::Path::new(&path))
            .await
            .map_err(|e| ErrorObject::owned(-32000, e.to_string(), None::<()>))?;
        
        Ok(nodes)
    }
    
    async fn subscribe_events(
        &self,
        mut subscription: jsonrpsee::server::SubscriptionSink,
    ) -> SubscriptionResult {
        let mut rx = self.event_bus.subscribe();
        
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if subscription.send(&event).await.is_err() {
                    break;
                }
            }
        });
        
        Ok(())
    }
    
    async fn emit_event(&self, event: String, data: String) -> Result<(), ErrorObject<'_>> {
        info!("events.emit called: {} {}", event, data);
        
        // Parse and forward to event bus
        // For MVP, just log
        Ok(())
    }
}
```

**Critère de succès:** Handlers connectés à VFS et event bus

---

### Après-midi (16:00 - 18:00) — Main et Demo

**Tâche C2.6: Main binary**
```rust
// src/bin/twake-sync.rs
use std::path::PathBuf;
use clap::Parser;
use tracing::info;

use twake_sync::ipc::server::start_unix_server;
use twake_sync::ipc::handlers::SyncEngineApi;
use twake_sync::events::bus::EventBus;
use twake_sync::models::InMemoryVfs;
use twake_sync::services::hydration::HydrationService;
use twake_sync::db::repository::FileRepository;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "/tmp/twake-ipc.sock")]
    socket: String,
    
    #[arg(short, long, default_value = "sqlite:twake.db")]
    database: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    tracing_subscriber::fmt::init();
    
    // Initialize components
    let vfs = Box::new(InMemoryVfs::new());
    let repo = FileRepository::new(&args.database).await?;
    let hydration = HydrationService::new(vfs.clone(), repo);
    let event_bus = EventBus::new();
    
    let api = SyncEngineApi {
        vfs,
        hydration,
        event_bus,
    };
    
    // Start IPC server
    let handle = start_unix_server(&args.socket).await?;
    
    info!("Sync engine running, press Ctrl+C to stop");
    
    // Keep running
    tokio::signal::ctrl_c().await?;
    
    info!("Shutting down...");
    handle.stop()?;
    
    Ok(())
}
```

**Tâche C2.7: Test client**
```rust
// src/bin/test-ipc.rs
use jsonrpsee::http_client::HttpClientBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClientBuilder::default()
        .build("http://localhost")?;
    
    // Test file.status
    let status: serde_json::Value = client
        .request("file.status", rpc_params!{"/test.txt"})
        .await?;
    
    println!("Status: {:?}", status);
    
    Ok(())
}
```

**Tâche C2.8: Demo script**
```bash
#!/bin/bash
# test-ipc.sh

echo "1. Starting IPC server..."
./target/release/twake-sync &
SERVER_PID=$!
sleep 2

echo "2. Testing file.status..."
curl -X POST http://localhost \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'

echo "3. Testing file.hydrate..."
curl -X POST http://localhost \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.hydrate","params":{"path":"/test.txt"},"id":2}'

echo "4. Stopping server..."
kill $SERVER_PID
```

**Critère de succès:** IPC server répond, events publiés

---

## Build Commands

```bash
# Build
cargo build --release

# Run IPC server
./target/release/twake-sync

# Test IPC
./target/release/test-ipc
```

## Dependencies

```toml
# Required
- jsonrpsee (JSON-RPC server)
- reqwest (HTTP client)
- tokio-tungstenite (WebSocket)
- base64, sha2, rand (PKCE)
- serde, serde_json (serialization)
- clap (CLI)
- tracing (logging)
```

## Known Issues

- **Unix socket permissions:** May need chmod
- **jsonrpsee subscriptions:** Complex, keep simple for MVP
- **Token refresh:** Not implemented for MVP
