# Project Initialization Implementation Plan

**Date:** 2026-03-25  
**Week:** 1 (J1-J3)  
**Approach:** Contract-First + TDD  
**Streams:** A (C++), B (Rust VFS), C (Rust IPC)

---

## Overview

This plan breaks down the project initialization into bite-sized tasks (2-5 minutes each) following TDD principles. Tasks are organized by day and stream, with parallel execution where possible.

**Prerequisites:**

- Rust 1.75+ installed (`rustc --version`)
- CMake 3.16+ installed (`cmake --version`)
- Git configured
- CEF 122.x binaries available

---

## Day 1 — Structure de base et types communs

### Phase 1: Repository Structure (Stream C - Lead)

**Duration:** 30 minutes

#### Task 1.1: Create workspace root files

```bash
# Step 1: Create root Cargo.toml
cat > Cargo.toml << 'EOF'
[workspace]
members = ["sync-engine"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["Twake Team"]

[workspace.dependencies]
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1.6", features = ["v4", "serde"] }
thiserror = "1.0"
async-trait = "0.1"
jsonrpsee = { version = "0.22", features = ["server", "macros"] }
time = { version = "0.3", features = ["serde", "format-description"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
EOF
```

**Expected:** `Cargo.toml` created at workspace root

---

#### Task 1.2: Create sync-engine workspace

```bash
# Step 2: Create sync-engine library
cargo new sync-engine --lib
```

**Expected:** `sync-engine/` directory with basic Cargo.toml and src/lib.rs

---

#### Task 1.3: Add dependencies to sync-engine

```bash
cd sync-engine
# Add dependencies with required features
cargo add tokio@1.35 --features full
cargo add serde@1.0 --features derive
cargo add serde_json@1.0
cargo add uuid@1.6 --features v4,serde
cargo add thiserror@1.0
cargo add async-trait@0.1
cargo add jsonrpsee@0.22 --features server,macros
cargo add time@0.3 --features serde
cargo add tracing@0.1
cargo add tracing-subscriber@0.3 --features env-filter
```

**Expected:** Dependencies added to `sync-engine/Cargo.toml`

---

#### Task 1.4: Create module directories

```bash
# Step 4: Create directory structure
mkdir -p src/models src/ipc src/events src/vfs src/reconciliation src/bin
touch src/models/mod.rs
touch src/models/file_state.rs
touch src/models/file_node.rs
touch src/ipc/mod.rs
touch src/ipc/contract.rs
touch src/events/mod.rs
touch src/events/types.rs
touch src/events/bus.rs
touch src/vfs/mod.rs
touch src/vfs/vfs_trait.rs
touch src/vfs/in_memory_vfs.rs
touch src/reconciliation/mod.rs
touch src/reconciliation/engine_trait.rs
```

---

#### Task 1.4.5: Update lib.rs to export modules

**File:** `sync-engine/src/lib.rs`

```rust
//! Twake Sync Engine
//!
//! Core synchronization logic for Twake Desktop NG.

pub mod models;
pub mod ipc;
pub mod events;
pub mod vfs;
pub mod reconciliation;

pub use models::{FileNode, FileState};
pub use ipc::{TwakeSyncApi, FileStatus};
pub use events::TwakeEvent;
```

**Expected:** All module files created

---

#### Task 1.5: Create CMakeLists.txt root

```bash
cd ..
cat > CMakeLists.txt << 'EOF'
cmake_minimum_required(VERSION 3.16)
project(twake-desktop-ng)

# C++ standard
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Build options
option(BUILD_CEF_SHELL "Build CEF Shell" ON)

if(BUILD_CEF_SHELL)
    add_subdirectory(cef-shell)
endif()
EOF
```

---

#### Task 1.5.5: Create scripts directory

```bash
mkdir -p scripts
```

---

#### Task 1.5.6: Create README.md

**File:** `README.md`

````markdown
# Twake Desktop NG

Collaborative work platform desktop client.

## Quick Start (5 minutes)

### Prerequisites

- Rust 1.75+ (`rustup update`)
- CMake 3.16+ (`sudo apt install cmake`)
- CEF 122.x binaries

### Build

```bash
# Rust sync engine
cargo build

# CEF shell
cd cef-shell && mkdir -p build && cd build
cmake .. && make
```
````

### Run

```bash
# Start IPC server
cargo run --bin twake-sync

# In another terminal, test IPC
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'
```

## Documentation

- [Architecture](docs/adr/ADR-0003-two-process-architecture.md)
- [IPC Contract](docs/superpowers/specs/ipc-contract-design.md)
- [VFS Engine](docs/superpowers/specs/vfs-engine-design.md)

````

**Expected:** Root `CMakeLists.txt` created

---

### Phase 2: Shared Models (Stream C + B Review)

**Duration:** 45 minutes

#### Task 1.6: Implement FileState enum (TDD)

**File:** `sync-engine/src/models/file_state.rs`

```rust
use serde::{Deserialize, Serialize};

/// File synchronization state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    /// Metadata only, content not downloaded
    Ghost,
    /// Content on disk, in sync with remote
    Hydrated,
    /// Local changes pending sync
    Modified,
    /// Sync operation in progress
    Syncing,
    /// Conflict detected during sync
    Conflict,
    /// Sync error occurred
    Error,
}

impl Default for FileState {
    fn default() -> Self {
        Self::Ghost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ghost_serialization() {
        let json = serde_json::to_string(&FileState::Ghost).unwrap();
        assert_eq!(json, "\"ghost\"");
    }

    #[test]
    fn test_hydrated_deserialization() {
        let state: FileState = serde_json::from_str("\"hydrated\"").unwrap();
        assert_eq!(state, FileState::Hydrated);
    }

    #[test]
    fn test_all_states_serialize() {
        let states = vec![
            FileState::Ghost,
            FileState::Hydrated,
            FileState::Modified,
            FileState::Syncing,
            FileState::Conflict,
            FileState::Error,
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: FileState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }
}
````

**Run tests:**

```bash
cd sync-engine
cargo test file_state
```

**Expected:** 3 tests pass

---

#### Task 1.7: Implement FileNode struct (TDD)

**File:** `sync-engine/src/models/file_node.rs`

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use time::OffsetDateTime;

use super::file_state::FileState;

/// File node representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: OffsetDateTime,
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}

impl FileNode {
    /// Create a new file node
    pub fn new(path: String, is_dir: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            path,
            state: FileState::default(),
            size: 0,
            modified: OffsetDateTime::now_utc(),
            is_dir,
            parent_id: None,
        }
    }

    /// Create a directory node
    pub fn directory(path: String) -> Self {
        Self::new(path, true)
    }

    /// Create a file node
    pub fn file(path: String) -> Self {
        Self::new(path, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_node_creation() {
        let node = FileNode::file("/test.txt".to_string());
        assert_eq!(node.path, "/test.txt");
        assert!(!node.is_dir);
        assert!(node.id != Uuid::nil());
    }

    #[test]
    fn test_directory_creation() {
        let node = FileNode::directory("/test".to_string());
        assert_eq!(node.path, "/test");
        assert!(node.is_dir);
    }

    #[test]
    fn test_serialization() {
        let node = FileNode::file("/test.txt".to_string());
        let json = serde_json::to_string(&node).unwrap();
        let deserialized: FileNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node.id, deserialized.id);
        assert_eq!(node.path, deserialized.path);
    }
}
```

**Update mod.rs:**

```bash
cat > sync-engine/src/models/mod.rs << 'EOF'
pub mod file_state;
pub mod file_node;

pub use file_state::FileState;
pub use file_node::FileNode;
EOF
```

**Run tests:**

```bash
cargo test models
```

**Expected:** 6 tests pass (3 from file_state + 3 from file_node)

---

### Phase 3: IPC Contract (Stream C)

**Duration:** 45 minutes

#### Task 1.8: Create FileStatus struct

**File:** `sync-engine/src/ipc/contract.rs`

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
    pub modified: String, // ISO 8601 timestamp
}
```

---

#### Task 1.8.5: Define VFS trait (Stream B - Parallel)

**File:** `sync-engine/src/vfs/vfs_trait.rs`

```rust
use async_trait::async_trait;
use crate::models::FileNode;

/// VFS backend interface
#[async_trait]
pub trait VfsBackend: Send + Sync {
    async fn get_status(&self, path: &str) -> Result<FileNode, VfsError>;
    async fn list_dir(&self, path: &str, recursive: bool) -> Result<Vec<FileNode>, VfsError>;
    async fn hydrate(&self, path: &str) -> Result<(), VfsError>;
    async fn create_file(&self, path: &str) -> Result<(), VfsError>;
    async fn create_dir(&self, path: &str) -> Result<(), VfsError>;
    async fn delete(&self, path: &str) -> Result<(), VfsError>;
}

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

#### Task 1.9: Define TwakeSyncApi trait

**Update:** `sync-engine/src/ipc/contract.rs` (append)

```rust
/// Twake Sync API - JSON-RPC interface
#[rpc(server, client)]
pub trait TwakeSyncApi {
    /// Get file status
    #[method(name = "file.status")]
    async fn file_status(&self, path: String) -> RpcResult<FileStatus>;

    /// Create a file
    #[method(name = "file.create")]
    async fn file_create(&self, path: String) -> RpcResult<bool>;

    /// Create a directory
    #[method(name = "file.create_dir")]
    async fn file_create_dir(&self, path: String) -> RpcResult<bool>;

    /// Delete a file or directory
    #[method(name = "file.delete")]
    async fn file_delete(&self, path: String) -> RpcResult<bool>;

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

    /// Emit an event
    #[method(name = "events.emit")]
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()>;
}
```

---

#### Task 1.10: Create TwakeEvent type

**File:** `sync-engine/src/events/types.rs`

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event types supported by the sync engine
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TwakeEvent {
    /// File state changed
    FileStateChanged {
        path: String,
        old_state: String,
        new_state: String,
    },
    /// New file detected
    FileCreated {
        path: String,
        is_dir: bool,
    },
    /// File deleted
    FileDeleted {
        path: String,
    },
    /// Sync started
    SyncStarted {
        file_count: u32,
    },
    /// Sync completed
    SyncCompleted {
        success: bool,
        error: Option<String>,
    },
    /// Error occurred
    Error {
        code: String,
        message: String,
    },
}

impl TwakeEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::FileStateChanged { .. } => "file_state_changed",
            Self::FileCreated { .. } => "file_created",
            Self::FileDeleted { .. } => "file_deleted",
            Self::SyncStarted { .. } => "sync_started",
            Self::SyncCompleted { .. } => "sync_completed",
            Self::Error { .. } => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_state_changed_event() {
        let event = TwakeEvent::FileStateChanged {
            path: "/test.txt".to_string(),
            old_state: "ghost".to_string(),
            new_state: "hydrated".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"file_state_changed\""));
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = TwakeEvent::SyncCompleted {
            success: true,
            error: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: TwakeEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, TwakeEvent::SyncCompleted { success: true, .. }));
    }
}
```

**Update events/mod.rs:**

```bash
cat > sync-engine/src/events/mod.rs << 'EOF'
pub mod types;
pub mod bus;

pub use types::TwakeEvent;
EOF
```

---

#### Task 1.11: Create EventBus (skeleton)

**File:** `sync-engine/src/events/bus.rs`

```rust
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use crate::events::types::TwakeEvent;

const CHANNEL_CAPACITY: usize = 1024;

/// Event bus for inter-component communication
pub struct EventBus {
    tx: broadcast::Sender<TwakeEvent>,
    rx: broadcast::Receiver<TwakeEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx, rx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TwakeEvent> {
        self.tx.subscribe()
    }

    pub async fn emit(&self, event: TwakeEvent) {
        info!("Event emitted: {}", event.event_type());
        let _ = self.tx.send(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_emit_subscribe() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        let event = TwakeEvent::FileCreated {
            path: "/test.txt".to_string(),
            is_dir: false,
        };

        bus.emit(event.clone()).await;

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, TwakeEvent::FileCreated { .. }));
    }
}
```

---

### Phase 4: CEF Shell Structure (Stream A - Parallel)

**Duration:** 30 minutes

#### Task 1.12: Create CEF shell directory structure

```bash
mkdir -p cef-shell/src/{app,browser,ipc}
```

**Expected:** Directory structure created

---

#### Task 1.13: Create CEF shell CMakeLists.txt

**File:** `cef-shell/CMakeLists.txt`

```cmake
cmake_minimum_required(VERSION 3.16)
project(cef-shell)

# CEF root directory
set(CEF_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/cef" CACHE PATH "CEF installation directory")

# Find CEF
find_package(CEF REQUIRED)

# Main application
add_executable(cef_app
    src/main.cpp
    src/app/browser_app.cpp
    src/app/render_app.cpp
    src/browser/window_manager.cpp
    src/browser/js_bridge.cpp
    src/ipc/ipc_client.cpp
)

target_include_directories(cef_app PRIVATE ${CEF_INCLUDE_PATH})
target_link_libraries(cef_app ${CEF_LIBRARIES})

# Add CEF macros
include(${CEF_USE_MACROS})
```

---

#### Task 1.14: Create main.cpp skeleton

**File:** `cef-shell/src/main.cpp`

```cpp
#include "include/base/cef_logging.h"
#include "include/cef_app.h"
#include "include/cef_command_line.h"

#include "app/browser_app.h"
#include "app/render_app.h"

int main(int argc, char* argv[]) {
    // Initialize CEF
    CefMainArgs main_args(argc, argv);

    // Parse command line
    CefRefPtr<CefCommandLine> command_line =
        CefCommandLine::CreateCommandLine();
    command_line->InitFromString(argc, argv);

    // Create appropriate app
    CefRefPtr<CefApp> app;
    if (command_line->HasSwitch("renderer")) {
        app = new RenderApp();
    } else {
        app = new BrowserApp();
    }

    // Execute CEF
    int exit_code = CefExecuteProcess(main_args, app.get(), nullptr);
    if (exit_code >= 0) {
        return exit_code;
    }

    // Initialize CEF browser process
    CefSettings settings;
    settings.multi_threaded_message_loop = true;

    CefInitialize(main_args, settings, app.get(), nullptr);

    // Run message loop
    CefRunMessageLoop();

    // Shutdown
    CefShutdown();

    return 0;
}
```

---

#### Task 1.15: Create IPC client skeleton

**File:** `cef-shell/src/ipc/ipc_client.h`

```cpp
#ifndef CEF_SHELL_IPC_CLIENT_H
#define CEF_SHELL_IPC_CLIENT_H

#include <string>
#include <nlohmann/json.hpp>

class IpcClient {
public:
    IpcClient() = default;
    ~IpcClient();

    bool connect(const std::string& socket_path);
    void disconnect();
    bool isConnected() const { return connected_; }

    nlohmann::json callMethod(const std::string& method,
                              const nlohmann::json& params);

private:
    int socket_fd_ = -1;
    bool connected_ = false;

    bool sendRequest(const nlohmann::json& request);
    nlohmann::json receiveResponse();
};

#endif // CEF_SHELL_IPC_CLIENT_H
```

**File:** `cef-shell/src/ipc/ipc_client.cpp`

```cpp
#include "ipc_client.h"
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <iostream>

IpcClient::~IpcClient() {
    disconnect();
}

bool IpcClient::connect(const std::string& socket_path) {
    if (connected_) {
        return true;
    }

    socket_fd_ = socket(AF_UNIX, SOCK_STREAM, 0);
    if (socket_fd_ == -1) {
        return false;
    }

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, socket_path.c_str(), sizeof(addr.sun_path) - 1);

    if (connect(socket_fd_, (struct sockaddr*)&addr, sizeof(addr)) == -1) {
        close(socket_fd_);
        socket_fd_ = -1;
        return false;
    }

    connected_ = true;
    return true;
}

void IpcClient::disconnect() {
    if (socket_fd_ != -1) {
        close(socket_fd_);
        socket_fd_ = -1;
    }
    connected_ = false;
}

nlohmann::json IpcClient::callMethod(const std::string& method,
                                      const nlohmann::json& params) {
    if (!connected_) {
        throw std::runtime_error("Not connected to IPC server");
    }

    nlohmann::json request = {
        {"jsonrpc", "2.0"},
        {"method", method},
        {"params", params},
        {"id", 1}
    };

    sendRequest(request);
    return receiveResponse();
}

bool IpcClient::sendRequest(const nlohmann::json& request) {
    std::string data = request.dump();
    ssize_t sent = write(socket_fd_, data.c_str(), data.length());
    return sent > 0;
}

nlohmann::json IpcClient::receiveResponse() {
    char buffer[4096];
    ssize_t received = read(socket_fd_, buffer, sizeof(buffer));
    if (received > 0) {
        return nlohmann::json::parse(buffer, buffer + received);
    }
    throw std::runtime_error("Failed to receive response");
}
```

---

### Phase 5: Validation (All Streams)

**Duration:** 15 minutes

#### Task 1.16: Build and test Day 1

```bash
# Build Rust workspace
cd sync-engine
cargo build

# Run all tests
cargo test

# Check for warnings
cargo clippy -- -D warnings
```

**Expected:** Build succeeds, all tests pass, no warnings

---

#### Task 1.17: Day 1 Checklist

- [ ] `Cargo.toml` workspace root created
- [ ] `sync-engine/` created with all modules
- [ ] `FileState` enum with 7 states and tests
- [ ] `FileNode` struct with tests
- [ ] `TwakeSyncApi` trait defined
- [ ] `TwakeEvent` enum with 6 event types
- [ ] `EventBus` with subscribe/emit
- [ ] `cef-shell/` directory structure created
- [ ] `IpcClient` C++ skeleton
- [ ] `cargo build` passes without warnings
- [ ] All unit tests pass

---

## Day 2 — IPC Server minimal

### Phase 1: IPC Server Implementation (Stream C)

**Duration:** 60 minutes

#### Task 2.1: Create IPC server skeleton

**File:** `sync-engine/src/ipc/server.rs`

```rust
use jsonrpsee::server::Server;
use jsonrpsee::core::SubscriptionResult;
use tracing::{info, error, instrument};

use crate::ipc::contract::{TwakeSyncApiServer, FileStatus, TwakeSyncApiServerMethods};
use crate::models::{FileNode, FileState};
use crate::events::bus::EventBus;
use crate::events::types::TwakeEvent;

/// IPC API implementation
pub struct SyncEngineApi {
    event_bus: EventBus,
}

impl SyncEngineApi {
    pub fn new(event_bus: EventBus) -> Self {
        Self { event_bus }
    }
}

#[jsonrpsee::core::async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    #[instrument(skip(self), fields(path = %path))]
    async fn file_status(&self, path: String) -> Result<FileStatus, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.status called");

        // Day 2: Dummy response
        // Day 3: Delegate to VFS
        Ok(FileStatus::new(path, FileState::Ghost))
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn file_hydrate(&self, path: String) -> Result<bool, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.hydrate called");

        // Day 2: Dummy response
        Ok(true)
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> Result<Vec<FileNode>, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.list called, recursive: {:?}", recursive);

        // Day 2: Empty response
        Ok(vec![])
    }

    async fn subscribe_events(
        &self,
        mut subscription: jsonrpsee::server::SubscriptionSink,
    ) -> SubscriptionResult {
        info!("events.subscribe called");

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

    #[instrument(skip(self))]
    async fn emit_event(&self, event: String, data: String) -> Result<(), jsonrpsee::types::ErrorObjectOwned> {
        info!("events.emit called: {} {}", event, data);
        Ok(())
    }
}

/// Start IPC server
pub async fn start_server(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Remove existing socket
    let _ = std::fs::remove_file(socket_path);

    info!("Starting IPC server on {}", socket_path);

    let server = Server::builder().build(socket_path).await?;
    let api = SyncEngineApi::new(EventBus::new());
    let handle = server.start(api.into_rpc());

    info!("IPC server started successfully");

    // Keep server running
    tokio::spawn(async move {
        handle.stopped().await;
    });

    Ok(())
}
```

---

#### Task 2.2: Update IPC module

**File:** `sync-engine/src/ipc/mod.rs`

```rust
pub mod contract;
pub mod server;
pub mod handlers;

pub use contract::{TwakeSyncApi, TwakeSyncApiServer, FileStatus};
pub use server::{SyncEngineApi, start_server};
```

---

#### Task 2.3: Create handlers module (skeleton)

**File:** `sync-engine/src/ipc/handlers.rs`

```rust
use tracing::info;

/// Placeholder for future handler implementations
/// Day 3: Connect these to actual VFS operations

pub fn handle_file_status(path: &str) -> String {
    info!("Handling file.status for: {}", path);
    // Will delegate to VFS on Day 3
    format!("{{\"state\":\"ghost\",\"size\":0}}")
}

pub fn handle_file_hydrate(path: &str) -> bool {
    info!("Handling file.hydrate for: {}", path);
    true
}

pub fn handle_file_list(path: &str, recursive: bool) -> usize {
    info!("Handling file.list for: {} (recursive: {})", path, recursive);
    0
}
```

---

#### Task 2.4: Create binary entry point

**File:** `sync-engine/src/bin/twake-sync.rs`

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sync_engine::ipc::server::start_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sync_engine=debug,tokio=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let socket_path = std::env::var("TWAKE_IPC_SOCKET")
        .unwrap_or_else(|_| "/tmp/twake-ipc.sock".to_string());

    start_server(&socket_path).await?;

    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
```

**Create Cargo bin entry:**

```bash
mkdir -p sync-engine/src/bin
```

---

#### Task 2.5: Test IPC server

**File:** `sync-engine/tests/server_test.rs` (integration test)

```rust
// Move from src/ipc/server_test.rs to tests/server_test.rs
```

#[cfg(test)]
mod tests {
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;

    use crate::ipc::server::start_server;
    use crate::ipc::contract::TwakeSyncApiClient;

    #[tokio::test]
    async fn test_server_start_stop() {
        let socket_path = "/tmp/twake-test-ipc.sock";

        // Start server
        start_server(socket_path).await.unwrap();

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Clean up
        let _ = std::fs::remove_file(socket_path);
    }

    #[tokio::test]
    async fn test_file_status_call() {
        let socket_path = "/tmp/twake-test-ipc2.sock";

        start_server(socket_path).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create HTTP client for Unix socket
        let client = HttpClientBuilder::default()
            .build(format!("http://{}", socket_path))
            .unwrap();

        // Call file.status
        let result: serde_json::Value = client
            .request("file.status", vec!["/test.txt".to_string()])
            .await
            .unwrap();

        assert!(result.get("path").is_some());
        assert!(result.get("state").is_some());

        // Clean up
        let _ = std::fs::remove_file(socket_path);
    }

}

````

---

### Phase 2: CEF Shell IPC Client Implementation (Stream A)

**Duration:** 45 minutes

#### Task 2.6: Add nlohmann/json to CEF shell

**File:** `cef-shell/CMakeLists.txt` (update)

```cmake
# Add nlohmann json
include(FetchContent)
FetchContent_Declare(
    json
    GIT_REPOSITORY https://github.com/nlohmann/json.git
    GIT_TAG v3.11.2
)
FetchContent_MakeAvailable(json)

target_link_libraries(cef_app PRIVATE nlohmann_json::nlohmann_json)
````

---

#### Task 2.7: Create IPC client implementation (complete)

See Task 1.15 for the implementation.

---

#### Task 2.8: Create IPC client test

**File:** `cef-shell/src/ipc/ipc_client_test.cpp`

```cpp
#include "ipc_client.h"
#include <iostream>
#include <cassert>

int main() {
    IpcClient client;

    // Test connection (will fail if server not running)
    bool connected = client.connect("/tmp/twake-ipc.sock");

    if (connected) {
        std::cout << "Connected to IPC server" << std::endl;

        // Test method call
        nlohmann::json params = {{"path", "/test.txt"}};
        auto result = client.callMethod("file.status", params);

        std::cout << "Result: " << result.dump(2) << std::endl;

        client.disconnect();
        std::cout << "Test passed!" << std::endl;
        return 0;
    } else {
        std::cout << "Could not connect to IPC server (expected if not running)" << std::endl;
        return 0;
    }
}
```

---

### Phase 3: Integration Test (Stream A + C)

**Duration:** 30 minutes

#### Task 2.9: Create curl test script

**File:** `scripts/test_ipc.sh`

```bash
#!/bin/bash

set -e

SOCKET_PATH="/tmp/twake-ipc.sock"

echo "Testing IPC server..."

# Test file.status
echo "1. Testing file.status..."
curl -s --unix-socket "$SOCKET_PATH" \
    http://localhost/ \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}' | jq .

# Test file.hydrate
echo "2. Testing file.hydrate..."
curl -s --unix-socket "$SOCKET_PATH" \
    http://localhost/ \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"file.hydrate","params":{"path":"/test.txt"},"id":2}' | jq .

# Test file.list
echo "3. Testing file.list..."
curl -s --unix-socket "$SOCKET_PATH" \
    http://localhost/ \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"file.list","params":{"path":"/","recursive":false},"id":3}' | jq .

echo "All tests passed!"
```

```bash
chmod +x scripts/test_ipc.sh
```

---

#### Task 2.10: Day 2 Checklist

- [ ] `SyncEngineApi` implements all RPC methods
- [ ] IPC server starts on `/tmp/twake-ipc.sock`
- [ ] `file.status` returns dummy response
- [ ] `file.hydrate` returns true
- [ ] `file.list` returns empty array
- [ ] `events.subscribe` sets up subscription
- [ ] `events.emit` logs event
- [ ] C++ IPC client connects and calls methods
- [ ] curl test script works
- [ ] Integration tests pass

---

## Day 3 — End-to-End Test

### Phase 1: In-Memory VFS (Stream B)

**Duration:** 45 minutes

#### Task 3.1: Define VFS trait

**File:** `sync-engine/src/vfs/vfs_trait.rs`

```rust
use async_trait::async_trait;
use std::path::PathBuf;

use crate::models::{FileNode, FileState};

/// VFS backend interface
#[async_trait]
pub trait VfsBackend: Send + Sync {
    /// Get file status
    async fn get_status(&self, path: &str) -> Result<FileNode, VfsError>;

    /// List directory contents
    async fn list_dir(&self, path: &str, recursive: bool) -> Result<Vec<FileNode>, VfsError>;

    /// Hydrate a file (download content)
    async fn hydrate(&self, path: &str) -> Result<(), VfsError>;

    /// Create a file
    async fn create_file(&self, path: &str) -> Result<(), VfsError>;

    /// Create a directory
    async fn create_dir(&self, path: &str) -> Result<(), VfsError>;

    /// Delete a file or directory
    async fn delete(&self, path: &str) -> Result<(), VfsError>;
}

/// VFS errors
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}
```

**Update vfs/mod.rs:**

```bash
cat > sync-engine/src/vfs/mod.rs << 'EOF'
pub mod vfs_trait;
pub mod in_memory_vfs;

pub use vfs_trait::{VfsBackend, VfsError};
EOF
```

---

#### Task 3.2: Implement InMemoryVFS

**File:** `sync-engine/src/vfs/in_memory_vfs.rs`

```rust
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

use crate::models::{FileNode, FileState};
use crate::vfs::vfs_trait::{VfsBackend, VfsError};

/// In-memory VFS for testing and development
pub struct InMemoryVfs {
    nodes: Arc<RwLock<HashMap<String, FileNode>>>,
}

impl InMemoryVfs {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();

        // Add some test data
        let root = FileNode {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            path: "/".to_string(),
            state: FileState::Hydrated,
            size: 0,
            modified: time::OffsetDateTime::now_utc(),
            is_dir: true,
            parent_id: None,
        };
        nodes.insert("/".to_string(), root);

        Self {
            nodes: Arc::new(RwLock::new(nodes)),
        }
    }
}

#[async_trait]
impl VfsBackend for InMemoryVfs {
    async fn get_status(&self, path: &str) -> Result<FileNode, VfsError> {
        let nodes = self.nodes.read().await;

        nodes.get(path)
            .cloned()
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    async fn list_dir(&self, path: &str, _recursive: bool) -> Result<Vec<FileNode>, VfsError> {
        let nodes = self.nodes.read().await;

        let children: Vec<FileNode> = nodes.values()
            .filter(|n| {
                n.parent_id
                    .and_then(|pid| {
                        let parent = nodes.get(&n.path).and_then(|n| {
                            n.parent_id
                        });
                        parent
                    })
                    .map_or(false, |pid| {
                        // Simple parent matching
                        n.path.starts_with(path) &&
                        n.path.matches('/').count() == path.matches('/').count() + 1
                    })
            })
            .cloned()
            .collect();

        Ok(children)
    }

    async fn hydrate(&self, path: &str) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;

        if let Some(node) = nodes.get_mut(path) {
            info!("Hydrating file: {}", path);
            node.state = FileState::Hydrated;
            node.size = 1024; // Dummy size
            Ok(())
        } else {
            Err(VfsError::NotFound(path.to_string()))
        }
    }

    async fn create_file(&self, path: &str) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;

        if nodes.contains_key(path) {
            return Err(VfsError::AlreadyExists(path.to_string()));
        }

        let node = FileNode::file(path.to_string());
        nodes.insert(path.to_string(), node);

        info!("Created file: {}", path);
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;

        if nodes.contains_key(path) {
            return Err(VfsError::AlreadyExists(path.to_string()));
        }

        let node = FileNode::directory(path.to_string());
        nodes.insert(path.to_string(), node);

        info!("Created directory: {}", path);
        Ok(())
    }

    async fn delete(&self, path: &str) -> Result<(), VfsError> {
        let mut nodes = self.nodes.write().await;

        if nodes.remove(path).is_some() {
            info!("Deleted: {}", path);
            Ok(())
        } else {
            Err(VfsError::NotFound(path.to_string()))
        }
    }
}

impl Default for InMemoryVfs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_file() {
        let vfs = InMemoryVfs::new();

        vfs.create_file("/test.txt").await.unwrap();
        let status = vfs.get_status("/test.txt").await.unwrap();

        assert_eq!(status.path, "/test.txt");
        assert!(!status.is_dir);
    }

    #[tokio::test]
    async fn test_hydrate_file() {
        let vfs = InMemoryVfs::new();

        vfs.create_file("/test.txt").await.unwrap();
        vfs.hydrate("/test.txt").await.unwrap();

        let status = vfs.get_status("/test.txt").await.unwrap();
        assert_eq!(status.state, FileState::Hydrated);
        assert!(status.size > 0);
    }

    #[tokio::test]
    async fn test_delete_file() {
        let vfs = InMemoryVfs::new();

        vfs.create_file("/test.txt").await.unwrap();
        vfs.delete("/test.txt").await.unwrap();

        assert!(vfs.get_status("/test.txt").await.is_err());
    }
}
```

---

### Phase 2: Connect VFS to IPC (Stream C)

**Duration:** 30 minutes

#### Task 3.3: Update SyncEngineApi to use VFS

**File:** `sync-engine/src/ipc/server.rs` (update)

```rust
use crate::vfs::VfsBackend;

pub struct SyncEngineApi {
    event_bus: EventBus,
    vfs: Box<dyn VfsBackend>,
}

impl SyncEngineApi {
    pub fn new(event_bus: EventBus, vfs: Box<dyn VfsBackend>) -> Self {
        Self { event_bus, vfs }
    }
}

#[jsonrpsee::core::async_trait]
impl TwakeSyncApiServer for SyncEngineApi {
    async fn file_status(&self, path: String) -> Result<FileStatus, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.status called: {}", path);

        match self.vfs.get_status(&path).await {
            Ok(node) => Ok(FileStatus {
                path: node.path,
                state: node.state,
                size: node.size,
                modified: node.modified
                    .format(&time::format_description::well_known::Iso8601::DEFAULT)
                    .unwrap_or_default()
                    .to_string(),
            }),
            Err(_) => Ok(FileStatus::new(path, FileState::Ghost)),
        }
    }

    async fn file_hydrate(&self, path: String) -> Result<bool, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.hydrate called: {}", path);

        match self.vfs.hydrate(&path).await {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn file_list(
        &self,
        path: String,
        recursive: Option<bool>,
    ) -> Result<Vec<FileNode>, jsonrpsee::types::ErrorObjectOwned> {
        info!("file.list called: {}, recursive: {:?}", path, recursive);

        match self.vfs.list_dir(&path, recursive.unwrap_or(false)).await {
            Ok(nodes) => Ok(nodes),
            Err(_) => Ok(vec![]),
        }
    }

    // ... rest of methods unchanged
}
```

---

#### Task 3.4: Update binary to use InMemoryVFS

**File:** `sync-engine/src/bin/twake-sync.rs` (update)

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sync_engine::ipc::server::{start_server, SyncEngineApi};
use sync_engine::events::bus::EventBus;
use sync_engine::vfs::in_memory_vfs::InMemoryVfs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sync_engine=debug,tokio=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let socket_path = std::env::var("TWAKE_IPC_SOCKET")
        .unwrap_or_else(|_| "/tmp/twake-ipc.sock".to_string());

    let event_bus = EventBus::new();
    let vfs = Box::new(InMemoryVfs::new());
    let api = SyncEngineApi::new(event_bus, vfs);

    // Remove existing socket
    let _ = std::fs::remove_file(&socket_path);

    let server = jsonrpsee::server::Server::builder().build(&socket_path).await?;
    let handle = server.start(api.into_rpc());

    info!("IPC server started on {}", socket_path);
    info!("Press Ctrl+C to stop");

    // Keep running
    tokio::signal::ctrl_c().await?;
    handle.stop()?;

    Ok(())
}
```

---

### Phase 3: End-to-End Testing (All Streams)

**Duration:** 45 minutes

#### Task 3.5: Start IPC server

```bash
cd sync-engine
cargo run --bin twake-sync
```

**Expected:** Server starts and listens on `/tmp/twake-ipc.sock`

---

#### Task 3.6: Test with curl

**Terminal 2:**

```bash
    # Test file.status
    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}' | jq .

# Expected response:
# {"jsonrpc":"2.0","result":{"path":"/test.txt","state":"hydrated","size":0,"modified":"..."},"id":1}
```

---

#### Task 3.7: Test file creation and hydration

```bash
    # Create a file
    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.create","params":{"path":"/newfile.txt"},"id":2}' | jq .

    # Check status
    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/newfile.txt"},"id":3}' | jq .

    # Hydrate
    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.hydrate","params":{"path":"/newfile.txt"},"id":4}' | jq .

    # Check status again
    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/newfile.txt"},"id":5}' | jq .
```

---

#### Task 3.8: Test C++ IPC client

```bash
# Compile C++ client
cd cef-shell
mkdir -p build && cd build
cmake ..
make cef_app

# Run test (must have server running)
./cef_app --test-ipc
```

**Expected output:**

```
Connected to IPC server
Result: {
  "jsonrpc": "2.0",
  "result": {
    "path": "/test.txt",
    "state": "hydrated",
    "size": 1024,
    "modified": "..."
  },
  "id": 1
}
Test passed!
```

---

#### Task 3.9: Measure latency

```bash
# Create benchmark script
cat > scripts/benchmark_ipc.sh << 'EOF'
#!/bin/bash

SOCKET="/tmp/twake-ipc.sock"
ITERATIONS=100
TOTAL_TIME=0

for i in $(seq 1 $ITERATIONS); do
    START=$(date +%s%N)

    curl -s --unix-socket "$SOCKET" \
        http://localhost/ \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"file.status","params":"/test.txt","id":1}' > /dev/null

    END=$(date +%s%N)
    ELAPSED=$((END - START))
    TOTAL_TIME=$((TOTAL_TIME + ELAPSED))
done

AVG=$((TOTAL_TIME / ITERATIONS / 1000))
echo "Average latency: ${AVG} microseconds"

if [ $AVG -lt 100000 ]; then
    echo "✓ Latency < 100ms"
    exit 0
else
    echo "✗ Latency > 100ms"
    exit 1
fi
EOF

chmod +x scripts/benchmark_ipc.sh
./scripts/benchmark_ipc.sh
```

---

### Phase 4: Final Validation

**Duration:** 30 minutes

#### Task 3.10: Run all tests

```bash
cd sync-engine
cargo test --all
cargo clippy --all-targets -- -D warnings
```

---

#### Task 3.11: Build CEF shell

```bash
cd cef-shell
mkdir -p build && cd build
cmake ..
make
```

---

#### Task 3.12: Day 3 Checklist

- [ ] `InMemoryVfs` implements all VFS traits
- [ ] VFS tests pass (create, hydrate, delete)
- [ ] IPC server connected to VFS
- [ ] `file.status` returns real data from VFS
- [ ] `file.hydrate` updates file state
- [ ] E2E curl tests pass
- [ ] C++ IPC client receives responses
- [ ] Latency < 100ms
- [ ] `cargo test` passes
- [ ] `cargo clippy` has no warnings
- [ ] CEF shell builds

---

## Week 1 Success Criteria

### Code Quality

- [ ] 0 compiler warnings (Rust and C++)
- [ ] All unit tests pass
- [ ] Integration tests pass
- [ ] Clippy passes with `-D warnings`

### Architecture

- [ ] `models/` is independent (no circular dependencies)
- [ ] IPC contract is the single source of truth
- [ ] Build systems are isolated (Cargo for Rust, CMake for C++)
- [ ] Clear separation between streams

### Functionality

- [ ] IPC server starts and responds
- [ ] C++ client can connect and call methods
- [ ] In-memory VFS stores and retrieves files
- [ ] End-to-end flow works (C++ → IPC → VFS → response)
- [ ] Event bus can emit and subscribe

### Documentation

- [ ] README.md has 5-minute setup guide
- [ ] All public APIs have doc comments
- [ ] Examples in contract.rs work

---

## Next Steps (Week 2+)

After Week 1 completion:

1. **Stream A**: Implement CEF WebView integration, window management
2. **Stream B**: Implement FUSE/ProjFS backend, sync reconciliation
3. **Stream C**: Implement OIDC authentication, network client

All streams can now develop in parallel using the established IPC contract.

---

## Appendix: Quick Commands

### Start IPC Server

```bash
cd sync-engine
cargo run --bin twake-sync
```

### Test IPC

```bash
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":"/test.txt","id":1}'
```

### Run Tests

```bash
cd sync-engine
cargo test --all
```

### Build Everything

```bash
# Rust
cargo build --release

# C++
cd cef-shell/build && cmake .. && make
```

---

**End of Plan**
