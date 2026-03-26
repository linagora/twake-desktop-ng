# Contrats d'Interface - Hackathon 48h

**Date:** 2026-03-25  
**Objectif:** Définir les interfaces entre les 3 streams pour travail parallèle

---

## 1. Contrat IPC (JSON-RPC)

**Responsable:** Stream C  
**Deadline:** Jour 1, 10:00 (2h après démarrage)

### Transport

- **Unix socket:** `/tmp/twake-ipc.sock` (Linux)
- **Protocol:** JSON-RPC 2.0
- **Encoding:** UTF-8

### Méthodes (6 méthodes pour MVP)

```json
{
  "methods": [
    {
      "name": "file.status",
      "params": {
        "path": "string"
      },
      "returns": "FileStatus",
      "description": "Get file state (Ghost/Hydrated/Modified/Syncing/Conflict/Error)"
    },
    {
      "name": "file.hydrate",
      "params": {
        "path": "string"
      },
      "returns": "{ success: boolean, error?: string }",
      "description": "Download file content from remote"
    },
    {
      "name": "file.list",
      "params": {
        "path": "string",
        "recursive": "boolean (default: false)"
      },
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
      "params": {
        "event": "string",
        "data": "string (JSON)"
      },
      "returns": "null",
      "description": "Emit event from WebView to sync engine"
    }
  ]
}
```

### Format des Requêtes

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

| Code   | Message                 |
| ------ | ----------------------- |
| -32000 | File not found          |
| -32001 | Permission denied       |
| -32002 | Network error           |
| -32003 | Authentication required |
| -32600 | Invalid request         |
| -32601 | Method not found        |

---

## 2. Types de Données Partagés

### FileNode (Rust ↔ C++)

> **Source de vérité pour les types partagés.** Les STREAM\_\*.md et les design specs
> doivent s'aligner sur ces définitions. Le modèle interne Rust utilise `Uuid` et
> `OffsetDateTime` ; la sérialisation JSON produit `String` et ISO 8601.

```rust
// Modèle interne Rust (sync-engine/src/models/)
// Sérialise en String/ISO 8601 sur le fil JSON-RPC automatiquement.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileNode {
    pub id: Uuid,              // UUID v4 (sérialisé en string JSON)
    pub remote_id: Option<String>, // ID côté serveur Cozy (ex: "6a1ff9c8...")
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

### Event Types (Rust → C++)

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
**Implémenté par:** Stream A  
**Consommé par:** WebApps Twake

```javascript
// Injecté dans chaque WebView Twake
window.__twake = {
  // Synchronous (blocking, use sparingly)
  getFileStatus(path: string): FileStatus,

  // Asynchronous (preferred)
  async hydrateFile(path: string): Promise<{ success: boolean, error?: string }>,
  async listFiles(path: string, recursive: boolean): Promise<FileNode[]>,
  async getToken(): Promise<{ access_token: string, expires_in: number }>,

  // Event subscription
  on(event: string, callback: (data: any) => void): void,
  off(event: string, callback: (data: any) => void): void,
};

// Exemple d'utilisation
const status = window.__twake.getFileStatus('/documents/test.txt');
console.log('State:', status.state); // "ghost" or "hydrated"

await window.__twake.hydrateFile('/documents/test.txt');
console.log('File downloaded!');

window.__twake.on('file_changed', (data) => {
  console.log('File changed:', data.path, data.state);
});
```

---

## 4. FUSE Mount Point

**Responsable:** Stream B  
**Deadline:** Jour 1, 14:00

### Mount Point

- **Path:** `/mnt/twake` (configurable)
- **Permissions:** 777 (MVP, pas de security)
- **User:** root (sudo pour mount)

### Structure

```
/mnt/twake/
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

**Stream A (C++):**

```
src/cef/
  app/
    browser_app.cpp/h
    render_app.cpp/h
  browser/
    window_manager.cpp/h
    js_bridge.cpp/h
  ipc/
    ipc_client.cpp/h
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

### Fonctions/Méthodes

**C++:** `camelCase` pour méthodes, `PascalCase` pour classes

```cpp
class JsBridge {
  void injectBridge(CefRefPtr<CefBrowser> browser);
  std::string getFileStatus(const std::string& path);
};
```

**Rust:** `snake_case` pour tout

```rust
fn get_file_status(path: &str) -> Result<FileStatus>;
async fn hydrate_file(path: &str) -> Result<()>;
```

**JavaScript:** `camelCase`

```javascript
getFileStatus(path);
hydrateFile(path);
```

---

## 6. Points de Synchronisation

### Jour 1 — 10:00 (2h après démarrage)

**Objectif:** Contrat IPC validé

**Stream C livre:**

- [ ] `contract.rs` avec JSON-RPC schema
- [ ] Types Rust (FileNode, FileState, Event)
- [ ] Exemple de requête/réponse

**Stream A vérifie:**

- [ ] Peut sérialiser/désérialiser les types
- [ ] IPC client peut appeler les méthodes

**Stream B vérifie:**

- [ ] FileNode correspond aux besoins FUSE
- [ ] Méthodes suffisantes pour VFS operations

---

### Jour 1 — 14:00 (6h après démarrage)

**Objectif:** IPC server + FUSE mount fonctionnels

**Stream C livre:**

- [ ] IPC server écoute sur `/tmp/twake-ipc.sock`
- [ ] Méthode `file.status` renvoie dummy data

**Stream B livre:**

- [ ] FUSE mount point `/mnt/twake` créé
- [ ] Placeholder files visibles dans mount point

**Test d'intégration:**

```bash
# Stream A appelle Stream C
curl -X POST --unix-socket /tmp/twake-ipc.sock \
  http://localhost/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"file.status","params":{"path":"/test.txt"},"id":1}'

# Stream B vérifie FUSE
ls -la /mnt/twake/
```

---

### Jour 1 — 18:00 (Fin de journée 1)

**Objectif:** End-to-end test

**Test:**

1. Stream A: WebView appelle `window.__twake.getFileStatus("/test.txt")`
2. Stream C: IPC server reçoit requête
3. Stream B: FUSE retourne FileNode
4. Stream C: Réponse remontée à Stream A
5. Stream A: Console JS affiche statut

**Critère de succès:**

- [ ] Test passe sans crash
- [ ] Réponse en < 100ms
- [ ] Aucun deadlock

---

### Jour 2 — 12:00 (Milieu journée 2)

**Objectif:** Hydrate fonctionnel

**Test:**

1. Stream A: WebView click sur ghost file
2. Stream A: `window.__twake.hydrateFile("/test.txt")`
3. Stream C: IPC appelle download
4. Stream B: FUSE écrit fichier sur disk
5. Stream A: File icon change

**Critère de succès:**

- [ ] Fichier télécharge et s'écrit
- [ ] State transition Ghost → Hydrated
- [ ] Fichier lisible après hydrate

---

## 7. Logs et Debugging

### Format de Logs

**Stream A (C++):**

```cpp
LOG(INFO) << "[CEF] Bridge injected for browser " << browser_id;
LOG(ERROR) << "[CEF] IPC connection failed: " << error_msg;
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
- **DEBUG:** Detailed flow (MVP: désactivé)

---

## 8. Backup Plans

### Si IPC échoue

**Fallback:** Communication via fichiers temporaires

```
/tmp/twake-ipc/
├── request.json
└── response.json
```

Polling toutes les 100ms (lent mais fonctionnel)

---

### Si FUSE échoue

**Fallback:** Dossier normal (pas VFS)

```
~/TwakeSync/
├── documents/
└── shared/
```

Files sont toujours hydratés (pas de placeholders)

---

### Si CEF échoue

**Fallback:** Electron (si npm install rapide)

```bash
npm install electron
# Utiliser Electron au lieu de CEF
```

---

## 9. Checklist de Validation

### Jour 1 — 23:00

- [ ] Contrat IPC écrit et validé par les 3 streams
- [ ] IPC server répond aux requêtes
- [ ] FUSE mount visible (`ls /mnt/twake`)
- [ ] Bridge JS injecté dans WebView
- [ ] IPC client peut appeler méthode
- [ ] End-to-end test passe

### Jour 2 — 23:00

- [ ] Auth OIDC fonctionne
- [ ] Ghost files visibles dans FUSE
- [ ] Hydrate télécharge fichier
- [ ] File state transitionne correctement
- [ ] Demo de 5 minutes sans crash

---

## 10. Contacts et Escalade

**Blockers > 30min:**

1. Essayer de résoudre soi-même
2. Slack channel #hackathon
3. Si pas de réponse en 15min → tous les devs se réunissent

**Changes aux interfaces:**

- Slack channel #hackathon
- Mettre à jour ce document
- Tous les streams doivent valider

**Points de sync obligatoires:**

- Jour 1, 10:00 — Contrat IPC
- Jour 1, 14:00 — IPC + FUSE
- Jour 1, 18:00 — End-to-end
- Jour 2, 12:00 — Hydrate
- Jour 2, 18:00 — Demo prep
