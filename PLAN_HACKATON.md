# Twake Desktop NG - Plan Hackathon 48h

**Date:** 2026-03-25  
**Durée:** 48 heures  
**Objectif:** POC fonctionnel démo-able  
**Équipe:** 3 développeurs + IA

---

## Philosophie du Hackathon

**Règle d'or :** POC démo-able, pas production

**Ce qu'on retire :**
- ❌ CI/CD - pas le temps
- ❌ Logging structuré - stderr suffit
- ❌ Security hardening - auth basique OK
- ❌ Tests automatisés - manuel OK
- ❌ Windows/macOS - Linux seulement
- ❌ Conflict resolution - last-write-wins simple
- ❌ Notifications - pas dans scope
- ❌ Auto-update - Phase 3

---

## Objectif de Démo

**Scénario de 5 minutes :**

1. Lancement de l'app (CEF shell)
2. Login OIDC (popup navigateur)
3. Fenêtre Twake Drive s'ouvre
4. Fichiers "ghosts" visibles dans FUSE mount
5. Click sur un ghost → download → fichier réel
6. WebView peut voir le statut du fichier

---

## Jour 1 — Infrastructure de Base

### Matin (08:00 - 12:00)

**Stream C — IPC Contract + Server**
- [ ] JSON-RPC schema minimal (3-4 méthodes max)
- [ ] IPC server (Unix socket, jsonrpsee)
- [ ] Event bus (tokio::broadcast basique)

**Stream B — Sync Core Models**
- [ ] FileNode struct (UUID, path, state)
- [ ] FileState enum (Ghost, Hydrated, Modified)
- [ ] VFS trait definition (get_node, create_placeholder, hydrate)

**Stream A — CEF Setup**
- [ ] CEF prebuilt binaries download
- [ ] CMakeLists.txt minimal
- [ ] CefInitialize + message loop
- [ ] Browser creation (1 window)

---

### Après-midi (14:00 - 18:00)

**Stream C — Event Bus**
- [ ] Publish/subscribe basique
- [ ] Event types (FileChanged, SyncStarted)
- [ ] Forwarding to CEF (CefProcessMessage)

**Stream B — FUSE Backend**
- [ ] FUSE 3.x mounting (fuse3 crate)
- [ ] Placeholder file creation
- [ ] readdir() → list FileNodes
- [ ] open() → trigger hydrate

**Stream A — Bridge + IPC Client**
- [ ] JS bridge injection (`window.__twake`)
- [ ] Domain filtering (only Twake domains)
- [ ] IPC client (Unix socket, JSON-RPC)
- [ ] Method calls: getFileStatus, hydrateFile

---

### Soir (19:00 - 23:00)

**Integration — POC Test**

**Objectif :** Stream A appelle Stream C → Stream B → réponse

- [ ] WebView appelle `window.__twake.getFileStatus("/test.txt")`
- [ ] IPC client envoie requête à Rust
- [ ] FUSE trait retourne FileNode (Ghost)
- [ ] Réponse remontée à WebView
- [ ] Console JS affiche statut

**Bug fixes critiques :**
- [ ] CEF crash → recovery
- [ ] IPC disconnect → retry
- [ ] FUSE mount failure → error message

---

## Jour 2 — Feature MVP

### Matin (08:00 - 12:00)

**Stream B — Hydration**
- [ ] hydrate() implémentation (download + write disk)
- [ ] File download (reqwest, simple GET)
- [ ] Placeholder → Hydrated state transition
- [ ] Error handling (network failure)

**Stream A — User Interaction**
- [ ] Click droit sur ghost → menu contextuel
- [ ] "Hydrate" option dans menu
- [ ] Progress indicator (console.log pour MVP)
- [ ] File icon changement après hydrate

**Stream C — Network Minimal**
- [ ] GET file metadata (Twake API)
- [ ] GET file content (download)
- [ ] Token header (Authorization: Bearer)
- [ ] Error codes (401, 404, 500)

---

### Après-midi (14:00 - 18:00)

**Stream C — OIDC PKCE**
- [ ] .well-known discovery (mock pour MVP)
- [ ] PKCE flow (code_verifier, code_challenge)
- [ ] Authorization code exchange
- [ ] Token storage (fichier JSON chiffré, pas keyring)

**Stream A — Login UI**
- [ ] Login button dans CEF shell
- [ ] Open browser pour OIDC
- [ ] Callback URL handler (localhost:port)
- [ ] Token receipt + storage

**Stream B — Sync Test**
- [ ] Sync d'un dossier test (10 fichiers max)
- [ ] Ghost creation pour chaque fichier
- [ ] Metadata sync (size, modified time)
- [ ] Progress tracking (console.log)

---

### Soir (19:00 - 23:00)

**Demo Prep**

**Setup de démo :**
- [ ] Script de lancement (1 commande)
- [ ] Données de test pré-configurées
- [ ] Scénario écrit (5 minutes max)
- [ ] Backup plan (screenshots si crash)

**Bug fixes :**
- [ ] Priorité 1 : crash CEF
- [ ] Priorité 2 : IPC disconnect
- [ ] Priorité 3 : FUSE mount failure
- [ ] Priorité 4 : OIDC flow

**Polish :**
- [ ] Logs clairs (ce qu'on voit en démo)
- [ ] Messages d'erreur explicites
- [ ] Timer de démo (répéter 3x minimum)

---

## Livrables Finaux

### Code

- [ ] Stream A : CEF shell fonctionnel (Linux)
- [ ] Stream B : FUSE + hydration fonctionnel
- [ ] Stream C : IPC + OIDC basique fonctionnel

### Démo

- [ ] Scénario noté (étape par étape)
- [ ] Setup documenté (1 page max)
- [ ] Backup plan (screenshots/vidéo)

### Documentation

- [ ] README.md (setup rapide, 5 minutes)
- [ ] Known issues (bugs connus, workaround)
- [ ] Next steps (après hackathon)

---

## Checklist Quotidienne

### Fin Jour 1 (23:00)

**Must have :**
- [ ] CEF window s'ouvre
- [ ] FUSE mount visible (ls /mnt/twake)
- [ ] IPC call fonctionne (WebView → Rust → réponse)
- [ ] Aucun crash critique

**Nice to have :**
- [ ] Event bus fonctionne
- [ ] Placeholder creation automatique

---

### Fin Hackathon (23:00 Jour 2)

**Must have :**
- [ ] Auth OIDC fonctionne
- [ ] Ghost files visibles dans FUSE
- [ ] Click → hydrate → fichier réel
- [ ] Demo de 5 minutes sans crash

**Nice to have :**
- [ ] Multiple fichiers sync
- [ ] Progress indicator
- [ ] Error handling propre

---

## Risques et Mitigations

| Risque | Impact | Mitigation |
|--------|--------|------------|
| CEF build échoue | High | Utiliser prébuilt binaries, skip build from source |
| FUSE mount rate | High | Fallback : dossier normal, pas FUSE |
| IPC disconnect | Medium | Retry logic simple, reconnect auto |
| OIDC trop complexe | Medium | Mock auth (hardcoded token) |
| Crash en démo | Critical | Backup plan (screenshots/vidéo) |

---

## Roles et Responsibilities

**Stream A — CEF Shell (Dev 1)**
- CEF setup, window management
- Bridge JS, IPC client
- Login UI, tray icon (optionnel)

**Stream B — Sync Core (Dev 2)**
- FileNode models, VFS trait
- FUSE backend, hydration
- Database (SQLite minimal)

**Stream C — IPC + Network (Dev 3)**
- JSON-RPC contract, IPC server
- Event bus, OIDC PKCE
- Network layer (download/upload)

**IA — Support**
- Code generation (boilerplate)
- Debugging assistance
- Documentation

---

## Setup Environnement

### Pré-requis

- [ ] Rust (cargo, rustup)
- [ ] C++ compiler (gcc/g++, CMake)
- [ ] CEF prebuilt binaries
- [ ] FUSE 3.x dev headers
- [ ] Git

### Commandes Rapides

```bash
# Build CEF
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build

# Build Rust
cargo build --release

# Run FUSE mount
sudo mkdir -p /mnt/twake
sudo chmod 777 /mnt/twake
./target/release/twake-vfs /mnt/twake

# Run CEF shell
./build/cef_app
```

---

## Notes

- **Focus sur la démo** — pas de perfectionnisme
- **Logs console** — suffisant pour debug
- **Hardcoded values** — OK pour MVP (URL, tokens)
- **1 plateforme** — Linux seulement
- **Pas de tests** — manuel OK
- **Documentation minimale** — README + scénario démo
