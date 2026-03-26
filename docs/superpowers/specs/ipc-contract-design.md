# IPC Contract Design Spec

**Date:** 2026-03-26
**Component:** Sync Engine (Rust) ↔ Electron Shell (TypeScript)
**Stream:** C - IPC + Network
**Status:** Draft

---

## Overview

The IPC (Inter-Process Communication) contract defines how the Electron shell (TypeScript) and sync engine (Rust sidecar) communicate. It uses JSON-RPC 2.0 over Unix sockets (Linux/macOS) or named pipes (Windows).

**Design principle:** Minimal, stable contract that enables parallel development.

---

## Architecture

```
┌─────────────────────────┐                    ┌──────────────────────┐
│  Electron Shell (TS)    │                    │  Sync Engine (Rust)  │
│                         │                    │                      │
│  ┌───────────────┐      │   JSON-RPC 2.0     │  ┌──────────────┐   │
│  │ SidecarManager│──────┼────────────────────┼──│ IPC Server   │   │
│  │ (net.Socket)  │      │   Unix Socket      │  │ (jsonrpsee)  │   │
│  │               │      │                    │  │              │   │
│  │ - file.*      │      │                    │  │ - file.*     │   │
│  │ - events.*    │      │                    │  │ - events.*   │   │
│  │ - auth.*      │      │                    │  │ - auth.*     │   │
│  └───────────────┘      │                    │  └──────────────┘   │
│                         │                    │                      │
│  ┌───────────────┐      │                    │  ┌──────────────┐   │
│  │ Preload.ts    │      │                    │  │  Event Bus   │   │
│  │ contextBridge │      │                    │  │              │   │
│  │ window.__twake│      │                    │  │              │   │
│  └───────────────┘      │                    │  └──────────────┘   │
└─────────────────────────┘                    └──────────────────────┘
```

**Data flow (renderer → Rust):**

```
Renderer (SPA)
  → window.__twake.hydrateFile('/test.txt')    [contextBridge]
  → ipcRenderer.invoke('twake:file:hydrate')   [Electron IPC]
  → Main process ipc-bridge.ts                 [ipcMain.handle]
  → SidecarManager.call('file.hydrate', {...}) [Unix socket]
  → Rust IPC server                            [jsonrpsee]
  → VFS hydrate                                [business logic]
  → JSON-RPC response                          [socket]
  → Promise resolved in SPA                    [back up the chain]
```

---

## Transport

### Unix Socket (Linux/macOS)

- **Path:** `$XDG_RUNTIME_DIR/twake-ipc.sock` or `~/.twake/ipc.sock` (fallback)
- **Permissions:** User-owned (600)
- **Protocol:** JSON-RPC 2.0 over stream socket, newline-delimited

### Named Pipe (Windows)

- **Path:** `\\.\pipe\twake-ipc`
- **Protocol:** JSON-RPC 2.0 over named pipe

### Connection Management

- **Client (Electron):** Connect on sidecar start, reconnect on failure
- **Server (Rust):** Listen for multiple clients (Electron + CLI tools)
- **Timeout:** 30 seconds for method calls

---

## Methods

### file.status

Get file state (Ghost/Hydrated/Modified/Syncing/Conflict/Error).

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "file.status",
  "params": { "path": "/documents/test.txt" },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "path": "/documents/test.txt",
    "state": "ghost",
    "size": 1024,
    "modified": "2026-03-25T10:00:00Z"
  },
  "id": 1
}
```

### file.hydrate

Download file content from remote.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "file.hydrate",
  "params": { "path": "/documents/test.txt" },
  "id": 2
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": { "success": true },
  "id": 2
}
```

### file.list

List directory contents.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "file.list",
  "params": { "path": "/documents", "recursive": false },
  "id": 3
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "path": "/documents/test.txt",
      "state": "ghost",
      "size": 1024,
      "modified": "2026-03-25T10:00:00Z",
      "is_dir": false,
      "remote_id": "6a1ff9c8e4b0a3d2",
      "parent_id": null
    }
  ],
  "id": 3
}
```

### auth.token

Get current authentication token.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "auth.token",
  "params": {},
  "id": 4
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "access_token": "eyJhbG...",
    "expires_in": 3600
  },
  "id": 4
}
```

### events.subscribe

Subscribe to event stream.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "events.subscribe",
  "params": [],
  "id": 5
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": { "subscription": 1 },
  "id": 5
}
```

**Events (push):**
```json
{
  "jsonrpc": "2.0",
  "method": "events.subscribe",
  "params": {
    "subscription": 1,
    "result": {
      "type": "file_changed",
      "path": "/documents/test.txt",
      "state": "hydrated"
    }
  }
}
```

### events.emit

Emit event from shell to sync engine.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "events.emit",
  "params": {
    "event": "file.edited",
    "data": "{\"path\":\"/documents/test.txt\"}"
  },
  "id": 6
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": null,
  "id": 6
}
```

---

## Event Types

### FileChanged

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanged {
    pub path: String,
    pub state: FileState,
    pub size: Option<u64>,
}
```

**When:** File state changes (Ghost → Hydrated, Modified → Syncing → Hydrated, etc.)

### SyncStarted

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStarted {
    pub path: String,
}
```

**When:** Sync operation begins

### SyncCompleted

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCompleted {
    pub path: String,
    pub duration_ms: u64,
    pub bytes_transferred: u64,
}
```

**When:** Sync operation completes successfully

### ConflictDetected

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictDetected {
    pub path: String,
    pub local_version: String,
    pub remote_version: String,
    pub backup_path: Option<String>,
}
```

**When:** Conflict detected during sync

### Error

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwakeError {
    pub code: i32,
    pub message: String,
    pub path: Option<String>,
}
```

**When:** Error occurs

---

## Data Types

### FileState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Ghost,
    Hydrated,
    Modified,
    Syncing,
    Conflict,
    Error,
}
```

### FileNode

> Aligne sur INTERFACES.md (source de verite).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: Uuid,
    pub remote_id: Option<String>,
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,     // ISO 8601
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}
```

### FileStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub state: FileState,
    pub size: u64,
    pub modified: String,
}
```

---

## Error Codes

| Code   | Message                  | Description |
|--------|--------------------------|-------------|
| -32000 | File not found           | Path does not exist |
| -32001 | Permission denied        | Access restricted |
| -32002 | Network error            | Remote sync failed |
| -32003 | Authentication required  | Token expired/invalid |
| -32004 | File locked              | Cannot modify locked file |
| -32005 | Disk full                | No space for hydration |
| -32600 | Invalid request          | Malformed JSON-RPC |
| -32601 | Method not found         | Unknown method |
| -32602 | Invalid params           | Wrong parameter types |
| -32700 | Parse error              | Invalid JSON |

**Error Response:**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "File not found"
  },
  "id": 1
}
```

---

## JavaScript Bridge API

```typescript
// Exposed via contextBridge.exposeInMainWorld('__twake', {...})
// in Electron preload script

window.__twake = {
  // Async (all operations go through main process to Rust)
  getFileStatus(path: string): Promise<FileStatus>,
  hydrateFile(path: string): Promise<{ success: boolean; error?: string }>,
  listFiles(path: string, recursive?: boolean): Promise<FileNode[]>,
  getToken(): Promise<{ access_token: string; expires_in: number } | null>,
  startAuth(): Promise<{ access_token: string; expires_in: number }>,

  // Event subscription
  on(event: string, callback: (data: any) => void): () => void,
};

// Example usage
const status = await window.__twake.getFileStatus('/documents/test.txt');
console.log('State:', status.state);

await window.__twake.hydrateFile('/documents/test.txt');

const unsub = window.__twake.on('file_changed', (data) => {
  console.log('File changed:', data.path, data.state);
});
```

---

## Implementation

### Rust Server (jsonrpsee)

```rust
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::SubscriptionResult;

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

    #[method(name = "auth.token")]
    async fn auth_token(&self) -> RpcResult<TokenInfo>;

    #[subscription(name = "events.subscribe", item = TwakeEvent)]
    async fn subscribe_events(&self) -> SubscriptionResult;

    #[method(name = "events.emit")]
    async fn emit_event(&self, event: String, data: String) -> RpcResult<()>;
}
```

### TypeScript Client (SidecarManager)

```typescript
class SidecarManager {
  private socket: Socket;
  private pending: Map<number, { resolve, reject }>;

  async call(method: string, params: object): Promise<unknown> {
    const id = ++this.requestId;
    const msg = JSON.stringify({ jsonrpc: '2.0', method, params, id }) + '\n';
    this.socket.write(msg);
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }
}
```

---

## Testing Strategy

### Unit Tests

- Serialization/deserialization
- Error code mapping
- Event type variants

### Integration Tests

- Method calls (request/response)
- Event subscriptions
- Reconnection logic
- Concurrent clients

### E2E Tests

- SPA → contextBridge → ipcRenderer → main → socket → Rust → response
- Event propagation (Rust → socket → main → ipcRenderer → SPA)
- Error handling (disconnect, timeout)

---

## Dependencies

```toml
[dependencies]
jsonrpsee = { version = "0.22", features = ["server", "macros", "ws-client"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.35", features = ["full"] }
thiserror = "1.0"
```

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Socket permissions** | High | User-owned socket, document setup |
| **Serialization bugs** | Medium | Comprehensive tests, version contract |
| **Subscription leaks** | Medium | Timeout, auto-unsubscribe |
| **Client disconnects** | Low | Reconnect logic, retry with backoff |
| **Contract instability** | High | Version contract, backward compatible |

---

## Versioning

**Contract version:** 1.0 (MVP)

**Backward compatibility:**
- New methods are additive
- Existing methods must not change signature
- Deprecation requires 2 versions notice

---

## References

- [STREAM_C_IPC_NETWORK.md](../../../STREAM_C_IPC_NETWORK.md) - Implementation guide
- [STREAM_A_ELECTRON.md](../../../STREAM_A_ELECTRON.md) - Electron shell + sidecar client
- [INTERFACES.md](../../../INTERFACES.md) - Interface contracts
- [jsonrpsee Documentation](https://docs.rs/jsonrpsee/)
- [JSON-RPC 2.0 Spec](https://www.jsonrpc.org/specification)
