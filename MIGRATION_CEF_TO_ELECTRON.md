# Migration CEF → Electron - Synthese

**Date:** 2026-03-26

---

## Documents crees (3)

- **`docs/adr/ADR-0004-electron-migration.md`** -- Decision architecturale complete avec :
  - 5 couches de securite (sandbox, context isolation, contextBridge, CSP, navigation restrictions)
  - Strategie performance (lazy windows, code caching, esbuild bundling, Electron built-ins)
  - Integration Rust sidecar (spawn + Unix socket JSON-RPC)
  - Comparatif CEF vs Electron mis a jour
- **`docs/superpowers/specs/electron-shell-design.md`** -- Design spec detaillee avec code TypeScript complet pour chaque composant (main, preload, protocol, sidecar, auth, tray)
- **`STREAM_A_ELECTRON.md`** -- Guide d'implementation jour par jour (remplace STREAM_A_CEF.md)

## Documents mis a jour (10)

- **`docs/spec.md`** (v5→v6) -- Architecture Electron, diagramme, tech stack, phases recentrees sur MVP auth+SPA
- **`docs/adr/ADR-0003`** -- Marque "Superseded by ADR-0004"
- **`docs/adr/README.md`** -- Index avec ADR-0004
- **`PLAN.md`** -- Stream A = Electron/TypeScript, risques mis a jour
- **`PLAN_HACKATON.md`** -- Scenario demo avec Electron + bridge
- **`INTERFACES.md`** -- Bridge API TypeScript, data flow renderer→main→Rust, conventions
- **`ipc-contract-design.md`** -- Client TypeScript, data flow complet
- **`project-initialization-design.md`** -- Structure repo Electron
- **`STREAM_C_IPC_NETWORK.md`** -- Note sur client TypeScript
- **`DOCS_REVIEW.md`** -- Review post-migration (8/10)

## Document supprime (1)

- **`STREAM_A_CEF.md`** -- Remplace par STREAM_A_ELECTRON.md

---

## Points cles architecture Electron

### Securite (5 couches)

1. **Sandbox ON** + `nodeIntegration: false` (defauts Electron 20+)
2. **Context isolation** + `contextBridge` (pas d'acces direct au preload)
3. **Whitelist de channels IPC** dans le main process
4. **Protocole custom** `twake://bundle/` avec CSP headers (pas de `file://`)
5. **Restrictions de navigation** (`will-navigate`, `setWindowOpenHandler`)

### Performance

- `show: false` + `ready-to-show` (pas de flash blanc)
- esbuild pour bundler le main process (elimine scan node_modules)
- Electron built-ins (`safeStorage`, `net.fetch`, `Notification`) au lieu de deps npm
- Lazy window creation

### MVP

1. SPA locale chargee via `twake://bundle/index.html`
2. Auth OIDC PKCE (fenetre dediee + callback HTTP local)
3. Token stocke via `safeStorage` (chiffrement OS)
4. Bridge `window.__twake` avec getToken, startAuth, getFileStatus, etc.
5. Sidecar Rust spawne et connecte via Unix socket JSON-RPC
