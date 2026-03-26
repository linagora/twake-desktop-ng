# Twake Desktop NG - Plan de Developpement

**Date:** 2026-03-26
**Version:** 2.0
**Equipe:** 3 developpeurs + IA
**Objectif:** Developpement parallele sans blocages mutuels

---

## Architecture du Decoupage

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
   │  Electron   │     │ Sync Core   │      │   IPC +     │
   │  Shell (TS) │     │  (Rust)     │      │  Network    │
   └─────────────┘     └─────────────┘      └─────────────┘
```

**Principe:** Chacun travaille independamment, seules les interfaces sont partagees.

---

## Stream A -- Electron Shell (TypeScript)

**Responsable:** Dev 1
**Stack:** TypeScript, Electron, esbuild

### Objectifs

- Shell Electron pour heberger les SPAs locales et WebViews Twake
- Gestion des fenetres natives (BrowserWindow)
- Tray icon et notifications OS
- Bridge JavaScript via contextBridge (`window.__twake`)
- Sidecar manager pour le sync engine Rust
- Authentification OIDC PKCE

### Livrables

#### A1. Infrastructure Electron

- [ ] Setup npm + TypeScript + esbuild
- [ ] BrowserWindow avec sandbox + contextIsolation
- [ ] Protocole custom `twake://bundle/` pour servir la SPA locale
- [ ] CSP headers via protocol handler
- [ ] Single instance lock

#### A2. Window Management

- [ ] Create/Close/Minimize/Maximize windows
- [ ] Show on `ready-to-show` (startup rapide)
- [ ] Navigation restrictions (protocoles autorises)
- [ ] Window open handler (deny par defaut)

#### A3. Native Integration

- [ ] Tray icon (Electron `Tray` API)
- [ ] Context menu
- [ ] Notifications (Electron `Notification` API)

#### A4. JavaScript Bridge (contextBridge)

- [ ] Preload script avec `contextBridge.exposeInMainWorld`
- [ ] Methodes exposees : getFileStatus, hydrateFile, listFiles, getToken, startAuth
- [ ] Event subscription (on/off avec unsubscribe)
- [ ] Validation des arguments (type checking)
- [ ] Channel whitelist dans le main process

#### A5. Sidecar Manager

- [ ] Spawn du binaire Rust au demarrage
- [ ] Connexion Unix socket
- [ ] JSON-RPC client
- [ ] Auto-restart en cas de crash
- [ ] Graceful shutdown

#### A6. Auth OIDC

- [ ] `.well-known` discovery
- [ ] PKCE flow avec fenetre auth dediee
- [ ] Callback HTTP local (127.0.0.1:random_port)
- [ ] Token storage chiffre via `safeStorage`
- [ ] Token refresh

### Dependances

**Bloquante:** Contrat IPC (JSON-RPC schema) -- 3 jours max d'attente

**Non bloquantes:**
- Peut preparer l'Electron shell pendant l'attente
- Peut developper le bridge avec mock IPC handlers

### Fichiers Source

```
electron-shell/
  package.json
  tsconfig.json
  electron-builder.yml
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

---

## Stream B -- Sync Core (Rust)

**Responsable:** Dev 2
**Stack:** Rust, tokio, FUSE, SQLite

(Inchange par rapport a la version precedente -- le sync engine Rust n'est pas affecte par la migration Electron)

### Objectifs

- Moteur de synchronisation VFS
- Gestion des fichiers placeholders
- Reconciliation avec strategie last-write-wins
- Base de donnees locale pour metadonnees

### Livrables

(Voir [STREAM_B_SYNC_CORE.md](STREAM_B_SYNC_CORE.md) pour le detail)

### Dependances

**AUCUNE** -- 100% independant

---

## Stream C -- IPC + Network (Rust)

**Responsable:** Dev 3
**Stack:** Rust, jsonrpsee, tokio, reqwest

(Essentiellement inchange -- le serveur IPC Rust communique via Unix socket, peu importe que le client soit en C++ ou TypeScript)

### Objectifs

- Definir le contrat IPC (JSON-RPC schema)
- Implementer le server IPC
- Gerer les evenements entre processus
- Authentification OIDC (cote serveur)
- Synchronisation reseau avec serveur Twake

### Livrables

(Voir [STREAM_C_IPC_NETWORK.md](STREAM_C_IPC_NETWORK.md) pour le detail)

### Dependances

**AUCUNE** -- Peut commencer immediatement

---

## Ordre d'Execution

### Semaine 1

```
Jour 1-3:
├─► Dev C: Ecrire contrat IPC (C1) -- PRIORITAIRE
├─► Dev B: Commencer Sync Core (B1, B2, B4)
└─► Dev A: Setup Electron (npm, BrowserWindow, protocol, preload)

Jour 4-5:
├─► Dev C: IPC Server (C2) + Event Bus (C3)
├─► Dev B: FUSE Backend (B3) + Reconciliation (B5)
└─► Dev A: Auth OIDC (A6) + Sidecar Manager (A5)
```

### Semaine 2-4

```
Stream A (Electron):
├─ Semaine 2: Sidecar manager, IPC bridge reel (plus de mock)
├─ Semaine 3: Multi-window, tray, notifications
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
├─► Connecter Stream A ↔ Stream C (Electron IPC → Unix socket → Rust)
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
    { "name": "file.status", "params": { "path": "string" }, "returns": "FileStatus" },
    { "name": "file.hydrate", "params": { "path": "string" }, "returns": "Result<void, Error>" },
    { "name": "file.list", "params": { "path": "string", "recursive": "boolean" }, "returns": "Vec<FileNode>" },
    { "name": "auth.token", "params": {}, "returns": "TokenInfo" },
    { "name": "events.subscribe", "params": {}, "returns": "Subscription" },
    { "name": "events.emit", "params": { "event": "string", "data": "string" }, "returns": "null" }
  ]
}
```

### Bridge API (JavaScript via contextBridge)

```typescript
window.__twake = {
  getFileStatus(path: string): Promise<FileStatus>,
  hydrateFile(path: string): Promise<{ success: boolean }>,
  listFiles(path: string, recursive?: boolean): Promise<FileNode[]>,
  getToken(): Promise<TokenInfo | null>,
  startAuth(): Promise<TokenInfo>,
  on(event: string, callback: (data: any) => void): () => void,
};
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

### Points de controle obligatoires

1. **Jour 3:** Contrat IPC valide par les 3 developpeurs
2. **Semaine 2:** Stream A peut appeler Stream C via Unix socket (test de bout en bout)
3. **Semaine 3:** Stream B peut emettre des evenements vers Stream C
4. **Semaine 5:** Integration complete, tests E2E

### Communication

- **Daily sync:** 15 min pour aligner sur les interfaces
- **Interface changes:** Discord + mise a jour du contrat IPC
- **Blockers:** Signaler immediatement, pas d'attente > 1 jour

---

## Risques et Mitigations

| Risque                | Impact | Mitigation                                        |
| --------------------- | ------ | ------------------------------------------------- |
| Electron perf (RAM)   | Medium | Lazy windows, code caching, monitoring            |
| IPC contract instable | Medium | Versionner le contrat, backward compatible        |
| VFS crash (FUSE)      | High   | Isoler dans processus separe, restart automatique |
| Conflict resolution   | Medium | Last-write-wins + backup (Phase 1)                |
| OIDC SSO complexity   | Medium | Mock SSO pour dev, vrai SSO pour prod             |
| Sidecar crash         | Medium | Auto-restart avec backoff                         |

---

## Checklist Globale

### Semaine 1

- [ ] Contrat IPC ecrit et valide (Dev C)
- [ ] Shell Electron fonctionnel avec SPA locale (Dev A)
- [ ] Sync Core models + trait (Dev B)

### Semaine 2-4

- [ ] Electron Shell avec auth OIDC et sidecar (Dev A)
- [ ] Sync Core VFS fonctionnel (Dev B)
- [ ] IPC Server + Network fonctionnel (Dev C)

### Semaine 5-6

- [ ] Integration complete
- [ ] Tests E2E
- [ ] Performance tuning
- [ ] MVP pret

---

## Notes

- **Pas d'attente:** Chaque stream peut avancer independamment
- **Interfaces stables:** Une fois le contrat IPC ecrit, ne pas le changer sans accord
- **Tests:** Chaque stream doit avoir ses tests unitaires
- **CI/CD:** Mettre en place apres Semaine 2 (quand les streams sont stables)
