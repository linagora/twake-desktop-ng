# Contrats d'Interface - Hackathon 48h

**Date:** 2026-03-26
**Objectif:** Definir les interfaces entre les 3 streams pour travail parallele

---

## 1. Contrat IPC (JSON-RPC)

**Responsable:** Stream C
**Deadline:** Jour 1, 10:00 (2h apres demarrage)

### Transport

- **Unix socket:** `/tmp/twake-ipc.sock` (Linux) -- ou `$XDG_RUNTIME_DIR/twake-ipc.sock`
- **Named pipe:** `\\.\pipe\twake-ipc` (Windows)
- **Protocol:** JSON-RPC 2.0
- **Encoding:** UTF-8

### Methodes (6 methodes pour MVP)

```json
{
  "methods": [
    {
      "name": "file.status",
      "params": { "path": "string" },
      "returns": "FileStatus",
      "description": "Get file state (Ghost/Hydrated/Modified/Syncing/Conflict/Error)"
    },
    {
      "name": "file.hydrate",
      "params": { "path": "string" },
      "returns": "{ success: boolean, error?: string }",
      "description": "Download file content from remote"
    },
    {
      "name": "file.list",
      "params": { "path": "string", "recursive": "boolean (default: false)" },
      "returns": "FileNode[]",
      "description": "List directory contents"
    },
    {
      "name": "auth.token",
      "params": {},
      "returns": "{ access_token: string, expires_in: number }",
      "description": "Get current auth token"
    },
    {
      "name": "events.subscribe",
      "params": {},
      "returns": "Subscription (JSON-RPC subscription ID)",
      "description": "Subscribe to event stream (FileChanged, SyncStarted, etc.)"
    },
    {
      "name": "events.emit",
      "params": { "event": "string", "data": "string (JSON)" },
      "returns": "null",
      "description": "Emit event from shell to sync engine"
    }
  ]
}
```

### Format des Requetes

**Request:**

```json
{
  "jsonrpc": "2.0",
  "method": "file.status",
  "params": { "path": "/documents/test.txt" },
  "id": 1
}
```

**Response (success):**

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

**Response (error):**

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

### Error Codes

| Code   | Message                |
| ------ | ---------------------- |
| -32000 | File not found         |
| -32001 | Permission denied      |
| -32002 | Network error          |
| -32003 | Authentication required|
| -32600 | Invalid request        |
| -32601 | Method not found       |

---

## 2. Types de Donnees Partages

### FileNode (Rust ↔ TypeScript)

> **Source de verite pour les types partages.** Les STREAM_*.md et les design specs
> doivent s'aligner sur ces definitions. Le modele interne Rust utilise `Uuid` et
> `OffsetDateTime` ; la serialisation JSON produit `String` et ISO 8601.

```rust
// Modele interne Rust (sync-engine/src/models/)
// Serialise en String/ISO 8601 sur le fil JSON-RPC automatiquement.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileNode {
    pub id: Uuid,              // UUID v4 (serialise en string JSON)
    pub remote_id: Option<String>, // ID cote serveur (ex: "6a1ff9c8...")
    pub path: String,          // Chemin relatif au mount point (/documents/test.txt)
    pub state: FileState,      // Enum value
    pub size: u64,             // Bytes (0 if Ghost)
    pub modified: String,      // ISO 8601 timestamp
    pub is_dir: bool,
    pub parent_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Ghost,        // Metadata only, not downloaded
    Hydrated,     // Content on disk, in sync
    Modified,     // Local changes pending sync
    Syncing,      // In progress
    Conflict,     // Conflict detected
    Error,        // Sync error
}
```

**Format JSON (sur le fil IPC):**

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "remote_id": "6a1ff9c8e4b0a3d2",
  "path": "/documents/test.txt",
  "state": "ghost",
  "size": 1024,
  "modified": "2026-03-25T10:00:00Z",
  "is_dir": false,
  "parent_id": null
}
```

### Event Types (Rust → Electron)

```rust
#[derive(Serialize, Deserialize, Debug)]
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

**Format JSON:**

```json
{
  "type": "file_changed",
  "path": "/documents/test.txt",
  "state": "hydrated"
}
```

---

## 3. Bridge JavaScript API

**Responsable:** Stream A
**Implemente par:** Stream A (preload.ts + contextBridge)
**Consomme par:** Local SPA et WebApps Twake

```typescript
// Expose via contextBridge.exposeInMainWorld('__twake', {...})
// Available dans le renderer comme window.__twake

interface TwakeBridge {
  // File operations (async — delegated to Rust via main process)
  getFileStatus(path: string): Promise<FileStatus>;
  hydrateFile(path: string): Promise<{ success: boolean; error?: string }>;
  listFiles(path: string, recursive?: boolean): Promise<FileNode[]>;

  // Authentication
  getToken(): Promise<{ access_token: string; expires_in: number } | null>;
  startAuth(): Promise<{ access_token: string; expires_in: number }>;

  // Event subscription (events pushed from Rust via main process)
  on(event: string, callback: (data: any) => void): () => void; // Returns unsubscribe function
}

// Usage
const status = await window.__twake.getFileStatus('/documents/test.txt');
console.log('State:', status.state); // "ghost" or "hydrated"

await window.__twake.hydrateFile('/documents/test.txt');
console.log('File downloaded!');

const unsubscribe = window.__twake.on('file_changed', (data) => {
  console.log('File changed:', data.path, data.state);
});
// Later: unsubscribe();
```

**Flux des donnees:**

```
Renderer (SPA)
  → window.__twake.hydrateFile('/test.txt')
  → contextBridge (preload.ts)
  → ipcRenderer.invoke('twake:file:hydrate', '/test.txt')
  → Main process (ipc-bridge.ts)
  → SidecarManager.call('file.hydrate', { path: '/test.txt' })
  → Unix socket JSON-RPC
  → Rust sync engine
  → VFS hydrate
  → Response JSON-RPC
  → Main process
  → ipcRenderer response
  → contextBridge
  → Promise resolved in SPA
```

---

## 4. FUSE Mount Point

**Responsable:** Stream B
**Deadline:** Jour 1, 14:00

### Mount Point

- **Path:** `~/TwakeSync` (configurable, default)
- **Permissions:** User-owned (fusermount3, pas de sudo)

### Structure

```
~/TwakeSync/
├── documents/
│   ├── test.txt          (Ghost → placeholder)
│   ├── photo.jpg         (Ghost → placeholder)
│   └── subdir/
│       └── file.md       (Ghost → placeholder)
└── shared/
    └── project/
        └── spec.md       (Hydrated → content on disk)
```

### Placeholder File Behavior

**Ghost file (placeholder):**

- Size: 0 bytes (or 64 bytes with metadata)
- open(): Trigger hydrate via IPC
- read(): Return error if not hydrated
- stat(): Return metadata from FileNode

**Hydrated file:**

- Size: Actual file size
- open(): Normal open
- read(): Return file content
- stat(): Return real metadata

---

## 5. Conventions de Nommage

### Fichiers Source

**Stream A (Electron/TypeScript):**

```
electron-shell/
  src/
    main.ts
    preload.ts
    windows.ts
    protocol.ts
    ipc-bridge.ts
    sidecar.ts
    auth.ts
    tray.ts
  renderer/
    index.html
    app.js
    styles.css
```

**Stream B (Rust):**

```
sync-engine/
  src/
    models/
      file_node.rs
      file_state.rs
    vfs/
      mod.rs
      fuse_backend.rs
```

**Stream C (Rust):**

```
sync-engine/
  src/
    ipc/
      mod.rs
      server.rs
      contract.rs
    events/
      mod.rs
      bus.rs
```

### Fonctions/Methodes

**TypeScript:** `camelCase` pour fonctions/methodes, `PascalCase` pour classes

```typescript
class SidecarManager {
  async call(method: string, params: object): Promise<unknown>;
}
```

**Rust:** `snake_case` pour tout

```rust
fn get_file_status(path: &str) -> Result<FileStatus>;
async fn hydrate_file(path: &str) -> Result<()>;
```

**JavaScript (SPA):** `camelCase`

```javascript
window.__twake.getFileStatus(path);
window.__twake.hydrateFile(path);
```

---

## 6. Points de Synchronisation

### Jour 1 -- 10:00 (2h apres demarrage)

**Objectif:** Contrat IPC valide

**Stream C livre:**

- [ ] `contract.rs` avec JSON-RPC schema
- [ ] Types Rust (FileNode, FileState, Event)
- [ ] Exemple de requete/reponse

**Stream A verifie:**

- [ ] Peut serialiser/deserialiser les types en TypeScript
- [ ] IPC bridge mock repond correctement

**Stream B verifie:**

- [ ] FileNode correspond aux besoins FUSE
- [ ] Methodes suffisantes pour VFS operations

---

### Jour 1 -- 14:00 (6h apres demarrage)

**Objectif:** IPC server + FUSE mount fonctionnels

**Stream C livre:**

- [ ] IPC server ecoute sur Unix socket
- [ ] Methode `file.status` renvoie dummy data

**Stream B livre:**

- [ ] FUSE mount point cree
- [ ] Placeholder files visibles dans mount point

**Test d'integration:**

```bash
# Stream A appelle Stream C
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'

# Stream B verifie FUSE
ls -la ~/TwakeSync/
```

---

### Jour 1 -- 18:00 (Fin de journee 1)

**Objectif:** End-to-end test

**Test:**

1. Stream A: SPA appelle `window.__twake.getFileStatus("/test.txt")`
2. Stream A: ipc-bridge.ts forward au sidecar (ou mock)
3. Stream C: IPC server recoit requete
4. Stream B: VFS retourne FileNode
5. Stream A: SPA affiche statut

**Critere de succes:**

- [ ] Test passe sans crash
- [ ] Reponse en < 100ms
- [ ] Aucun deadlock

---

### Jour 2 -- 12:00 (Milieu journee 2)

**Objectif:** Auth + Hydrate fonctionnels

**Test:**

1. Stream A: Clic sur "Login" dans la SPA
2. Stream A: OIDC PKCE flow (fenetre auth)
3. Stream A: Token recupere et affiche
4. Stream A: `window.__twake.hydrateFile("/test.txt")`
5. Stream C: IPC appelle download
6. Stream B: VFS ecrit fichier sur disque

**Critere de succes:**

- [ ] Auth OIDC fonctionne
- [ ] Token affiche dans la SPA
- [ ] Fichier telecharge et ecrit
- [ ] State transition Ghost → Hydrated

---

## 7. Logs et Debugging

### Format de Logs

**Stream A (TypeScript):**

```typescript
console.log('[electron:main] Bridge injected for window', windowId);
console.error('[electron:sidecar] Connection failed:', error);
```

**Stream B/C (Rust):**

```rust
info!("[FUSE] Mount point created: {}", path);
error!("[IPC] Method not found: {}", method);
```

### Niveaux de Logs

- **ERROR:** Crash, disconnect, auth failure
- **WARN:** Retry, fallback, slow operation
- **INFO:** Startup, shutdown, key operations
- **DEBUG:** Detailed flow (MVP: desactive)

---

## 8. Backup Plans

### Si IPC echoue

**Fallback:** Communication via fichiers temporaires

```
/tmp/twake-ipc/
├── request.json
└── response.json
```

Polling toutes les 100ms (lent mais fonctionnel)

---

### Si FUSE echoue

**Fallback:** Dossier normal (pas VFS)

```
~/TwakeSync/
├── documents/
└── shared/
```

Files sont toujours hydrates (pas de placeholders)

---

### Si Electron echoue

**Fallback:** Pas de fallback necessaire -- Electron est le choix technologique consolide.

---

## 9. Checklist de Validation

### Jour 1 -- 23:00

- [ ] Contrat IPC ecrit et valide par les 3 streams
- [ ] IPC server repond aux requetes
- [ ] FUSE mount visible (`ls ~/TwakeSync`)
- [ ] contextBridge injecte dans la SPA
- [ ] Sidecar manager peut se connecter
- [ ] End-to-end test passe

### Jour 2 -- 23:00

- [ ] Auth OIDC fonctionne
- [ ] Token visible dans la SPA
- [ ] Ghost files visibles dans FUSE
- [ ] Hydrate telecharge fichier
- [ ] File state transitionne correctement
- [ ] Demo de 5 minutes sans crash

---

## 10. Contacts et Escalade

**Blockers > 30min:**

1. Essayer de resoudre soi-meme
2. Slack channel #hackathon
3. Si pas de reponse en 15min → tous les devs se reunissent

**Changes aux interfaces:**

- Slack channel #hackathon
- Mettre a jour ce document
- Tous les streams doivent valider

**Points de sync obligatoires:**

- Jour 1, 10:00 -- Contrat IPC
- Jour 1, 14:00 -- IPC + FUSE
- Jour 1, 18:00 -- End-to-end
- Jour 2, 12:00 -- Auth + Hydrate
- Jour 2, 18:00 -- Demo prep
