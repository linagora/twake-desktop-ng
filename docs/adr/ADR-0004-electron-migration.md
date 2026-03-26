# ADR-0004: Migration from CEF to Electron

## Status

Accepted — Supersedes [ADR-0003](ADR-0003-two-process-architecture.md)

## Context

ADR-0003 chose CEF (Chromium Embedded Framework) for the desktop shell, primarily for security (no Node.js exposure) and smaller footprint. After further evaluation, several factors motivate a switch to Electron:

1. **Developer velocity** — The team has stronger TypeScript than C++ expertise. CEF's C++ shell (even 500-1000 lines) introduces build complexity (CMake, prebuilt binaries) and debugging friction that slows iteration.
2. **Ecosystem maturity** — Electron provides battle-tested APIs for auto-update, crash reporting, native menus, tray, notifications, and installers. CEF requires reimplementing each from scratch.
3. **Security parity** — Electron has caught up since ADR-0003 was written: sandbox is ON by default (v20+), context isolation ON by default (v12+), and `contextBridge` provides a safe, auditable bridge API. With proper configuration, Electron's security posture matches CEF.
4. **Performance manageability** — Electron's RAM overhead (~150-200MB idle vs CEF's ~100-120MB) is acceptable for a desktop app, and can be reduced via lazy window creation, V8 code caching, and main process bundling.
5. **Proven at scale** — VS Code, Slack, Discord, Figma desktop all ship Electron in production with millions of users.

**What remains unchanged:**
- Two-process architecture (shell + Rust sync engine)
- Rust sync engine (100% reusable, untouched)
- IPC contract (JSON-RPC over Unix socket / named pipe)
- JavaScript bridge API (`window.__twake`)
- Data models (FileNode, FileState, events)

## Decision

Replace the CEF Shell (C++) with an **Electron Shell (TypeScript)**, maintaining the two-process architecture:

```
┌─────────────────────────────────────────────────────────────┐
│               Electron Shell (TypeScript)                    │
│                                                             │
│  ┌─────────────────────┐   ┌─────────────────────┐         │
│  │    Main Process      │   │  Renderer Processes  │         │
│  │  (Node.js runtime)   │   │  (Chromium sandbox)  │         │
│  │                      │   │                      │         │
│  │  - Window management │   │  ┌──────┐ ┌──────┐  │         │
│  │  - IPC to Rust       │   │  │Drive │ │ Mail │  │         │
│  │  - Tray, menus       │   │  │ SPA  │ │ SPA  │  │         │
│  │  - Auth flow         │   │  └──────┘ └──────┘  │         │
│  │  - Sidecar lifecycle │   │                      │         │
│  └─────────────────────┘   │  contextBridge only   │         │
│           │                 │  (no Node access)     │         │
│           │ preload.ts      └─────────────────────┘         │
│           │ (contextBridge)                                  │
└───────────┼─────────────────────────────────────────────────┘
            │ Unix socket / JSON-RPC 2.0
            ▼
┌─────────────────────────────────────────────────────────────┐
│                  Sync Engine (Rust sidecar)                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    VFS       │  │ Reconcile    │  │   Network    │       │
│  │  (FUSE/      │  │   Engine     │  │   Layer      │       │
│  │   ProjFS)    │  │              │  │              │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

### Security Architecture

Electron's security model relies on **defense in depth** with multiple layers:

**Layer 1: Process isolation**
- Renderer processes run in Chromium's sandbox (enabled by default since Electron 20)
- `nodeIntegration: false` (default) — renderers cannot access Node.js APIs
- `contextIsolation: true` (default since Electron 12) — preload scripts run in isolated world

**Layer 2: Controlled bridge via contextBridge**
- `contextBridge.exposeInMainWorld()` exposes only whitelisted, validated functions
- Bridge functions are proxied — renderer cannot access the preload's scope
- Each exposed function validates arguments before forwarding to main process

**Layer 3: IPC channel validation**
- Main process validates every IPC message against a whitelist of allowed channels
- IPC handlers sanitize parameters before forwarding to Rust engine
- No arbitrary code execution paths from renderer to main

**Layer 4: Navigation and content restrictions**
- `webSecurity: true` (default) — enforces same-origin policy
- Restrict navigation to trusted origins only (`will-navigate` handler)
- Custom `twake://` protocol for serving local SPA (avoids `file://` risks)
- CSP headers injected via custom protocol handler

**Layer 5: Local SPA serving**
- Local HTML/JS served via registered `twake://bundle/` protocol
- Protocol handler resolves paths within the app bundle only (no path traversal)
- CSP: `default-src 'self' twake:; script-src 'self' twake:; connect-src https: wss:`

```typescript
// Main process — security configuration
const win = new BrowserWindow({
  webPreferences: {
    sandbox: true,                    // Chromium sandbox ON
    contextIsolation: true,           // Isolated preload world
    nodeIntegration: false,           // No Node in renderer
    preload: path.join(__dirname, 'preload.js'),
    webSecurity: true,                // Same-origin policy
  },
});

// Restrict navigation
win.webContents.on('will-navigate', (event, url) => {
  const allowed = ['twake://', 'https://sso.'];
  if (!allowed.some(prefix => url.startsWith(prefix))) {
    event.preventDefault();
  }
});

// Restrict window creation
win.webContents.setWindowOpenHandler(({ url }) => {
  if (url.startsWith('https://sso.')) {
    return { action: 'allow' };
  }
  return { action: 'deny' };
});
```

### Performance Strategy

**Startup optimization:**
- Create windows with `show: false`, display on `ready-to-show` event
- Bundle main process with esbuild (eliminates `node_modules` scan at runtime)
- V8 code caching for preload and main process scripts

**Memory optimization:**
- Lazy window creation (create windows only when user opens them)
- Use Electron built-in modules instead of npm packages:
  - `net.fetch` instead of `node-fetch`
  - `safeStorage` instead of `keytar` for token encryption
  - `Notification` instead of `node-notifier`
- Single shared preload script (not duplicated per window type)

**IPC optimization:**
- Structured clone algorithm (default since Electron 8) — ~2x faster than JSON for typed arrays
- MessagePort for direct renderer-to-utility communication (bypass main process)
- Batch small IPC calls where possible

**Bundle size target:**
- Electron binary: ~65-90MB (incompressible)
- App code (bundled): < 5MB
- Total installer: ~100-120MB
- Techniques: ASAR packaging, tree shaking, no dev dependencies in prod

### Rust Sidecar Integration

The Rust sync engine runs as a **sidecar process**, spawned and managed by Electron's main process:

```typescript
import { spawn } from 'child_process';
import { app } from 'electron';
import path from 'path';

function spawnSyncEngine(): ChildProcess {
  const binaryPath = path.join(
    process.resourcesPath, 'bin',
    process.platform === 'win32' ? 'twake-sync.exe' : 'twake-sync'
  );

  const socketPath = path.join(app.getPath('userData'), 'twake-ipc.sock');

  const child = spawn(binaryPath, ['--socket', socketPath], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  child.on('exit', (code) => {
    if (code !== 0) {
      // Auto-restart on crash (with backoff)
      setTimeout(() => spawnSyncEngine(), 1000);
    }
  });

  return child;
}
```

**Communication:** JSON-RPC 2.0 over Unix socket (unchanged from CEF architecture).

The IPC path is:
```
Renderer → (contextBridge) → Preload → (ipcRenderer.invoke) → Main → (Unix socket) → Rust
```

## Rationale

### Updated Comparison

| Criteria | CEF (ADR-0003) | Electron (this ADR) |
|----------|----------------|---------------------|
| Shell language | C++ | TypeScript |
| Build system | CMake + prebuilt binaries | npm + electron-builder |
| Install size | ~100MB | ~100-120MB |
| RAM idle | ~100-120MB | ~150-200MB |
| Node.js exposure | None | Sandboxed (no renderer access) |
| Security (configured) | High | High (sandbox + contextIsolation) |
| Auto-update | Manual implementation | electron-updater (built-in) |
| Crash reporting | Manual implementation | crashReporter (built-in) |
| Installer/packaging | Manual | electron-builder (all platforms) |
| Dev iteration speed | Slow (C++ compile) | Fast (hot reload possible) |
| Developer pool | Narrow (C++) | Wide (TypeScript) |

### Why the Trade-off is Acceptable

1. **RAM (+50-80MB):** Acceptable for a desktop collaboration platform. VS Code operates at ~300MB and users consider it lightweight.
2. **Bundle size (+20MB):** Negligible difference on modern networks and SSDs.
3. **Node.js in main process:** Mitigated by sandbox in renderers. Main process is trusted code we control, same as C++ was trusted code.

## Consequences

### Positive

1. **Faster development** — TypeScript is faster to write, debug, and iterate than C++
2. **Richer ecosystem** — electron-builder, electron-updater, crashReporter, Tray, Menu, Notification APIs ready to use
3. **Easier hiring** — TypeScript developers are far more common than C++ desktop developers
4. **Better DX** — Hot reload in development, Chrome DevTools, rich debugging
5. **Cross-platform packaging** — electron-builder produces .deb, .rpm, .AppImage, .dmg, .msi from single config

### Negative

1. **Higher memory baseline** — ~150-200MB vs ~100-120MB for CEF
2. **Larger bundle** — ~100-120MB vs ~100MB for CEF (marginal)
3. **Node.js attack surface** — Main process has Node.js access (mitigated by not exposing it to renderers)

### Risks

1. **Performance regression** — Mitigated by lazy loading, code caching, main process bundling
2. **Security misconfiguration** — Mitigated by security checklist and defaults (sandbox ON, contextIsolation ON)
3. **Electron version churn** — Mitigated by using LTS releases

## Migration Impact

### What Changes

| Component | Before (CEF) | After (Electron) |
|-----------|-------------|-------------------|
| Shell code | C++ (~500-1000 lines) | TypeScript (~200-400 lines) |
| Build system | CMake | npm + esbuild + electron-builder |
| Bridge injection | `CefV8Value` in `OnContextCreated` | `contextBridge.exposeInMainWorld` in preload |
| Window management | Native OS APIs | `BrowserWindow` API |
| IPC to renderers | `CefProcessMessage` | `ipcMain` / `ipcRenderer` |
| IPC to Rust | Unix socket (C++ client) | Unix socket (Node.js client) |
| Tray icon | Platform-specific C++ | `Tray` API |
| Notifications | Platform-specific C++ | `Notification` API |
| Token storage | File-based (C++) | `safeStorage` API |

### What Does NOT Change

- Rust sync engine (100% unchanged)
- IPC contract (JSON-RPC methods, events, error codes)
- JavaScript bridge API (`window.__twake`)
- Data models (FileNode, FileState)
- VFS implementation (FUSE/ProjFS/FileProvider)
- Authentication flow (OIDC PKCE)
- Reconciliation engine

## References

- [Electron Security](https://www.electronjs.org/docs/latest/tutorial/security)
- [Electron Performance](https://www.electronjs.org/docs/latest/tutorial/performance)
- [contextBridge API](https://www.electronjs.org/docs/latest/api/context-bridge)
- [ADR-0003](ADR-0003-two-process-architecture.md) — Previous architecture decision (superseded)
- [docs/spec.md](../spec.md) — Updated technical specification
