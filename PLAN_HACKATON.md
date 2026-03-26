# Twake Desktop NG - Plan Hackathon 48h

**Date:** 2026-03-26
**Duree:** 48 heures
**Objectif:** MVP fonctionnel demo-able
**Equipe:** 3 developpeurs + IA

---

## Philosophie du Hackathon

**Regle d'or :** MVP demo-able, pas production

**Ce qu'on retire :**

- CI/CD - pas le temps
- Logging structure - console.log suffit
- Security hardening avance - les defaults Electron suffisent (sandbox ON, contextIsolation ON)
- Tests automatises - manuel OK
- Windows/macOS - Linux seulement
- Conflict resolution - last-write-wins simple
- Notifications - pas dans scope
- Auto-update - Phase 3

---

## Objectif de Demo

**Scenario de 5 minutes :**

1. Lancement de l'app (Electron shell)
2. SPA locale se charge via `twake://bundle/`
3. Login OIDC (fenetre auth dediee)
4. Token recupere et affiche dans la SPA
5. SPA appelle `window.__twake.getFileStatus()` via le bridge
6. SPA recoit des donnees du sync engine (ou mock)

---

## Jour 1 -- Infrastructure de Base

### Matin (08:00 - 12:00)

**Stream C -- IPC Contract + Server**

- [ ] JSON-RPC schema minimal (4-5 methodes)
- [ ] IPC server (Unix socket, jsonrpsee)
- [ ] Event bus (tokio::broadcast basique)

**Stream B -- Sync Core Models**

- [ ] FileNode struct (UUID, path, state)
- [ ] FileState enum (Ghost, Hydrated, Modified, Syncing, Conflict, Error)
- [ ] VFS trait definition (get_node, create_placeholder, hydrate)

**Stream A -- Electron Setup**

- [ ] `npm init` + install Electron + TypeScript + esbuild
- [ ] BrowserWindow creation (sandbox ON, contextIsolation ON)
- [ ] Protocole `twake://bundle/` + SPA locale (index.html)
- [ ] Preload script avec contextBridge skeleton

---

### Apres-midi (14:00 - 18:00)

**Stream C -- Event Bus**

- [ ] Publish/subscribe basique
- [ ] Event types (FileChanged, SyncStarted)

**Stream B -- FUSE Backend**

- [ ] FUSE 3.x mounting (fuse3 crate)
- [ ] Placeholder file creation
- [ ] readdir() → list FileNodes

**Stream A -- Bridge + IPC Mock**

- [ ] contextBridge.exposeInMainWorld complete
- [ ] IPC handlers mock (file.status, file.list, auth.token)
- [ ] SPA peut appeler `window.__twake.*` et recevoir des reponses

---

### Soir (19:00 - 23:00)

**Integration -- POC Test**

**Objectif :** SPA appelle bridge → main process → (mock ou Rust) → reponse

- [ ] SPA appelle `window.__twake.getFileStatus("/test.txt")`
- [ ] Main process repond (mock ou via sidecar)
- [ ] SPA affiche le resultat
- [ ] Console JS montre le statut

**Bug fixes critiques :**

- [ ] Electron crash → check webPreferences
- [ ] Bridge non disponible → verifier preload
- [ ] Protocol handler 404 → verifier path resolution

---

## Jour 2 -- Auth + Integration

### Matin (08:00 - 12:00)

**Stream A -- Auth OIDC**

- [ ] AuthService avec PKCE (code_verifier, code_challenge)
- [ ] Fenetre BrowserWindow dediee pour SSO
- [ ] Serveur HTTP local pour callback (127.0.0.1:random)
- [ ] Token storage via safeStorage
- [ ] `window.__twake.startAuth()` fonctionne bout en bout

**Stream A -- Sidecar Manager**

- [ ] spawn() du binaire Rust
- [ ] Connexion Unix socket
- [ ] JSON-RPC client basique
- [ ] Fallback mock si sidecar pas disponible

**Stream B -- Hydration**

- [ ] hydrate() implementation (download + write disk)
- [ ] Placeholder → Hydrated state transition

**Stream C -- Network Minimal**

- [ ] GET file metadata
- [ ] GET file content (download)
- [ ] Token header (Authorization: Bearer)

---

### Apres-midi (14:00 - 18:00)

**Integration complete**

- [ ] SPA → bridge → main → sidecar → Rust IPC → reponse
- [ ] Auth OIDC end-to-end (ou mock SSO)
- [ ] Token affiche dans SPA
- [ ] Bridge calls atteignent le sidecar Rust

---

### Soir (19:00 - 23:00)

**Demo Prep**

**Setup de demo :**

- [ ] Script de lancement (1 commande)
- [ ] Donnees de test pre-configurees
- [ ] Scenario ecrit (5 minutes max)
- [ ] Backup plan (screenshots si crash)

**Bug fixes :**

- [ ] Priorite 1 : crash Electron
- [ ] Priorite 2 : bridge non fonctionnel
- [ ] Priorite 3 : auth OIDC
- [ ] Priorite 4 : sidecar connection

**Polish :**

- [ ] Logs clairs (ce qu'on voit en demo)
- [ ] Messages d'erreur explicites
- [ ] Timer de demo (repeter 3x minimum)

---

## Livrables Finaux

### Code

- [ ] Stream A : Electron shell fonctionnel (Linux)
- [ ] Stream B : FUSE + hydration fonctionnel
- [ ] Stream C : IPC + OIDC basique fonctionnel

### Demo

- [ ] Scenario note (etape par etape)
- [ ] Setup documente (1 page max)
- [ ] Backup plan (screenshots/video)

### Documentation

- [ ] README.md (setup rapide, 5 minutes)
- [ ] Known issues (bugs connus, workaround)
- [ ] Next steps (apres hackathon)

---

## Checklist Quotidienne

### Fin Jour 1 (23:00)

**Must have :**

- [ ] Fenetre Electron s'ouvre avec SPA locale
- [ ] `window.__twake` disponible dans la SPA
- [ ] Bridge calls fonctionnent (au moins en mock)
- [ ] FUSE mount visible (ls ~/TwakeSync)
- [ ] IPC server repond aux requetes

**Nice to have :**

- [ ] Event bus fonctionne
- [ ] Sidecar Rust connecte

---

### Fin Hackathon (23:00 Jour 2)

**Must have :**

- [ ] Auth OIDC fonctionne (ou mock convaincant)
- [ ] Token visible dans la SPA
- [ ] Bridge calls atteignent le sidecar (ou mock)
- [ ] Demo de 5 minutes sans crash

**Nice to have :**

- [ ] Hydrate via bridge
- [ ] Events Rust → SPA
- [ ] Tray icon

---

## Risques et Mitigations

| Risque                | Impact   | Mitigation                                             |
| --------------------- | -------- | ------------------------------------------------------ |
| Electron setup echoue | Low      | npm install est fiable, fallback: electron-quick-start |
| Sidecar pas pret      | Medium   | Mock IPC handlers dans le main process                 |
| OIDC trop complexe    | Medium   | Mock auth (token hardcode)                             |
| Protocol handler bugs | Medium   | Fallback: loadFile() au lieu de twake://               |
| Crash en demo         | Critical | Backup plan (screenshots/video)                        |

---

## Roles et Responsibilities

**Stream A -- Electron Shell (Dev 1)**

- Electron setup, window management, protocol handler
- Bridge contextBridge, IPC handlers
- Auth OIDC, sidecar manager

**Stream B -- Sync Core (Dev 2)**

- FileNode models, VFS trait
- FUSE backend, hydration
- Database (SQLite minimal)

**Stream C -- IPC + Network (Dev 3)**

- JSON-RPC contract, IPC server
- Event bus, OIDC PKCE (cote Rust)
- Network layer (download/upload)

**IA -- Support**

- Code generation (boilerplate)
- Debugging assistance
- Documentation

---

## Setup Environnement

### Pre-requis

- [ ] Node.js 20+ (npm)
- [ ] Rust (cargo, rustup)
- [ ] FUSE 3.x dev headers (`apt install libfuse3-dev`)
- [ ] Git

### Commandes Rapides

```bash
# Build Electron
cd electron-shell
npm install
npm run build

# Build Rust
cd sync-engine
cargo build --release

# Run FUSE mount
mkdir -p ~/TwakeSync
./target/release/twake-vfs --mount ~/TwakeSync

# Run Electron shell
cd electron-shell
npm start
```

---

## Notes

- **Focus sur la demo** -- pas de perfectionnisme
- **Mock first** -- tous les handlers commencent en mock, remplacer par reel progressivement
- **Security by default** -- sandbox ON, contextIsolation ON, pas de nodeIntegration
- **1 plateforme** -- Linux seulement
- **Pas de tests automatises** -- manuel OK
- **Documentation minimale** -- README + scenario demo
