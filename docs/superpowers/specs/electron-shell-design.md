# Electron Shell Design Spec

**Date:** 2026-03-26
**Component:** Electron Shell (TypeScript)
**Stream:** A - Electron Shell
**Status:** Draft

---

## Overview

The Electron Shell provides the desktop application framework that hosts Twake web applications and local SPAs in secure BrowserWindows. It manages windows, injects a JavaScript bridge via `contextBridge`, handles authentication, and communicates with the Rust sync engine sidecar via Unix socket.

**Key principle:** Minimal TypeScript code (~200-400 lines), security by default (sandbox, context isolation, CSP).

---

## Architecture

```
┌───────────────────────────────────────────────────────────────┐
│                     Electron Shell                             │
│                                                               │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │                    Main Process                           │ │
│  │                                                          │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │ │
│  │  │  WindowMgr   │  │  AuthService │  │  SidecarMgr  │   │ │
│  │  │              │  │  (OIDC PKCE) │  │  (Rust spawn)│   │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘   │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │ │
│  │  │  IpcBridge   │  │  Protocol    │  │  TrayService │   │ │
│  │  │  (to Rust)   │  │  (twake://)  │  │              │   │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘   │ │
│  └──────────────────────────────────────────────────────────┘ │
│                            │                                   │
│                    preload.ts (contextBridge)                  │
│                            │                                   │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │               Renderer Processes (sandboxed)              │ │
│  │                                                          │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐               │ │
│  │  │ Local    │  │ Drive    │  │ Calendar │               │ │
│  │  │ SPA      │  │ WebView  │  │ WebView  │               │ │
│  │  │ twake:// │  │ https:// │  │ https:// │               │ │
│  │  └──────────┘  └──────────┘  └──────────┘               │ │
│  │                                                          │ │
│  │  window.__twake exposed via contextBridge                │ │
│  └──────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. Main Process Entry Point

```typescript
// src/main.ts
import { app, BrowserWindow, protocol } from 'electron';
import { registerTwakeProtocol } from './protocol';
import { createMainWindow } from './windows';
import { setupIpcHandlers } from './ipc-bridge';
import { SidecarManager } from './sidecar';
import { AuthService } from './auth';

// Enforce single instance
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
}

app.whenReady().then(async () => {
  // Register custom protocol before creating windows
  registerTwakeProtocol();

  // Spawn Rust sync engine sidecar
  const sidecar = new SidecarManager();
  await sidecar.start();

  // Setup IPC handlers (Electron IPC ↔ Rust IPC bridge)
  const auth = new AuthService();
  setupIpcHandlers(sidecar, auth);

  // Create main window
  createMainWindow();
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
```

### 2. Window Management

```typescript
// src/windows.ts
import { BrowserWindow, shell } from 'electron';
import path from 'path';

export function createMainWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    show: false, // Show on ready-to-show for faster perceived startup
    webPreferences: {
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js'),
      webSecurity: true,
    },
  });

  // Show when ready (avoids white flash)
  win.once('ready-to-show', () => win.show());

  // Load local SPA via custom protocol
  win.loadURL('twake://bundle/index.html');

  // Security: restrict navigation
  win.webContents.on('will-navigate', (event, url) => {
    const parsed = new URL(url);
    const allowed = ['twake:', 'https:'];
    if (!allowed.includes(parsed.protocol)) {
      event.preventDefault();
    }
  });

  // Security: restrict new window creation
  win.webContents.setWindowOpenHandler(({ url }) => {
    // Open external links in system browser
    if (url.startsWith('https://')) {
      shell.openExternal(url);
    }
    return { action: 'deny' };
  });

  return win;
}

export function createAuthWindow(authUrl: string): BrowserWindow {
  const win = new BrowserWindow({
    width: 500,
    height: 700,
    show: false,
    webPreferences: {
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      // No preload needed — auth window is a plain browser
    },
  });

  win.once('ready-to-show', () => win.show());
  win.loadURL(authUrl);

  return win;
}
```

### 3. Custom Protocol (twake://)

Serves local SPA files securely, avoiding `file://` protocol risks.

```typescript
// src/protocol.ts
import { protocol, net } from 'electron';
import path from 'path';
import { pathToFileURL } from 'url';

export function registerTwakeProtocol() {
  protocol.handle('twake', (request) => {
    const url = new URL(request.url);

    if (url.hostname !== 'bundle') {
      return new Response('Not found', { status: 404 });
    }

    // Resolve path within app bundle (prevent path traversal)
    let filePath = decodeURIComponent(url.pathname);
    if (filePath === '/' || filePath === '') {
      filePath = '/index.html';
    }

    const resolved = path.resolve(
      path.join(__dirname, '..', 'renderer'),
      filePath.replace(/^\//, '')
    );

    // Security: ensure resolved path is within bundle directory
    const bundleDir = path.resolve(path.join(__dirname, '..', 'renderer'));
    if (!resolved.startsWith(bundleDir)) {
      return new Response('Forbidden', { status: 403 });
    }

    // Serve file with CSP headers
    const response = net.fetch(pathToFileURL(resolved).toString());
    return response.then(res => {
      const headers = new Headers(res.headers);
      headers.set('Content-Security-Policy', [
        "default-src 'self' twake:",
        "script-src 'self' twake:",
        "style-src 'self' twake: 'unsafe-inline'",
        "connect-src https: wss: twake:",
        "img-src 'self' twake: https: data:",
        "font-src 'self' twake: https:",
      ].join('; '));
      return new Response(res.body, {
        status: res.status,
        headers,
      });
    });
  });
}
```

### 4. Preload Script (contextBridge)

The preload script is the **only** bridge between renderer and main process. It exposes a strictly typed, validated API.

```typescript
// src/preload.ts
import { contextBridge, ipcRenderer } from 'electron';

// Expose window.__twake to renderer
contextBridge.exposeInMainWorld('__twake', {
  // File operations (delegated to Rust via main process)
  getFileStatus(path: string) {
    if (typeof path !== 'string') throw new TypeError('path must be string');
    return ipcRenderer.invoke('twake:file:status', path);
  },

  hydrateFile(path: string) {
    if (typeof path !== 'string') throw new TypeError('path must be string');
    return ipcRenderer.invoke('twake:file:hydrate', path);
  },

  listFiles(path: string, recursive = false) {
    if (typeof path !== 'string') throw new TypeError('path must be string');
    return ipcRenderer.invoke('twake:file:list', path, recursive);
  },

  // Authentication
  getToken() {
    return ipcRenderer.invoke('twake:auth:token');
  },

  startAuth() {
    return ipcRenderer.invoke('twake:auth:start');
  },

  // Events (Rust → renderer)
  on(event: string, callback: (...args: any[]) => void) {
    if (typeof event !== 'string') throw new TypeError('event must be string');
    const channel = `twake:event:${event}`;
    const listener = (_event: any, ...args: any[]) => callback(...args);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  },
});
```

### 5. IPC Bridge (Electron ↔ Rust)

The main process bridges Electron IPC (from renderers) to JSON-RPC calls (to Rust sidecar).

```typescript
// src/ipc-bridge.ts
import { ipcMain, BrowserWindow } from 'electron';
import { SidecarManager } from './sidecar';
import { AuthService } from './auth';

// Allowed IPC channels (whitelist)
const ALLOWED_CHANNELS = [
  'twake:file:status',
  'twake:file:hydrate',
  'twake:file:list',
  'twake:auth:token',
  'twake:auth:start',
] as const;

export function setupIpcHandlers(sidecar: SidecarManager, auth: AuthService) {
  // File operations → delegate to Rust via JSON-RPC
  ipcMain.handle('twake:file:status', async (_event, path: string) => {
    return sidecar.call('file.status', { path });
  });

  ipcMain.handle('twake:file:hydrate', async (_event, path: string) => {
    return sidecar.call('file.hydrate', { path });
  });

  ipcMain.handle('twake:file:list', async (_event, path: string, recursive: boolean) => {
    return sidecar.call('file.list', { path, recursive });
  });

  // Auth operations
  ipcMain.handle('twake:auth:token', async () => {
    return auth.getToken();
  });

  ipcMain.handle('twake:auth:start', async () => {
    return auth.startInteractiveAuth();
  });

  // Forward events from Rust to all renderers
  sidecar.onEvent((event: string, data: any) => {
    for (const win of BrowserWindow.getAllWindows()) {
      win.webContents.send(`twake:event:${event}`, data);
    }
  });
}
```

### 6. Rust Sidecar Manager

```typescript
// src/sidecar.ts
import { spawn, ChildProcess } from 'child_process';
import { app } from 'electron';
import { createConnection, Socket } from 'net';
import path from 'path';

export class SidecarManager {
  private process: ChildProcess | null = null;
  private socket: Socket | null = null;
  private socketPath: string;
  private requestId = 0;
  private pending = new Map<number, { resolve: Function; reject: Function }>();
  private eventListeners: Array<(event: string, data: any) => void> = [];

  constructor() {
    this.socketPath = path.join(app.getPath('userData'), 'twake-ipc.sock');
  }

  async start(): Promise<void> {
    const binaryName = process.platform === 'win32' ? 'twake-sync.exe' : 'twake-sync';
    const binaryPath = app.isPackaged
      ? path.join(process.resourcesPath, 'bin', binaryName)
      : path.join(__dirname, '..', '..', 'sync-engine', 'target', 'release', binaryName);

    this.process = spawn(binaryPath, ['--socket', this.socketPath], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    this.process.stderr?.on('data', (data) => {
      console.error('[sync-engine]', data.toString());
    });

    this.process.on('exit', (code) => {
      console.error(`Sync engine exited with code ${code}`);
      if (code !== 0) {
        setTimeout(() => this.start(), 2000); // Auto-restart with backoff
      }
    });

    // Wait for socket to be ready, then connect
    await this.waitForSocket();
    await this.connect();
  }

  async call(method: string, params: Record<string, unknown>): Promise<unknown> {
    const id = ++this.requestId;
    const request = JSON.stringify({
      jsonrpc: '2.0',
      method,
      params,
      id,
    }) + '\n';

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket!.write(request);

      // Timeout after 30s
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`RPC timeout: ${method}`));
        }
      }, 30_000);
    });
  }

  onEvent(listener: (event: string, data: any) => void) {
    this.eventListeners.push(listener);
  }

  stop() {
    this.socket?.destroy();
    this.process?.kill();
  }

  private async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.socket = createConnection(this.socketPath);

      let buffer = '';
      this.socket.on('data', (chunk) => {
        buffer += chunk.toString();
        const lines = buffer.split('\n');
        buffer = lines.pop()!;

        for (const line of lines) {
          if (!line.trim()) continue;
          try {
            const msg = JSON.parse(line);
            this.handleMessage(msg);
          } catch { /* ignore malformed */ }
        }
      });

      this.socket.once('connect', resolve);
      this.socket.once('error', reject);
    });
  }

  private handleMessage(msg: any) {
    if (msg.id && this.pending.has(msg.id)) {
      const { resolve, reject } = this.pending.get(msg.id)!;
      this.pending.delete(msg.id);
      if (msg.error) {
        reject(new Error(msg.error.message));
      } else {
        resolve(msg.result);
      }
    } else if (msg.method?.startsWith('events.')) {
      // Event notification from Rust
      const data = msg.params?.result || msg.params;
      const eventType = data?.type || msg.method;
      for (const listener of this.eventListeners) {
        listener(eventType, data);
      }
    }
  }

  private waitForSocket(): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        const sock = createConnection(this.socketPath);
        sock.once('connect', () => { sock.destroy(); resolve(); });
        sock.once('error', () => setTimeout(check, 100));
      };
      setTimeout(check, 200); // Give sidecar time to create socket
    });
  }
}
```

### 7. Authentication Service (OIDC PKCE)

```typescript
// src/auth.ts
import { BrowserWindow, safeStorage } from 'electron';
import { createServer } from 'http';
import { randomBytes, createHash } from 'crypto';

interface TokenResponse {
  access_token: string;
  token_type: string;
  expires_in: number;
  refresh_token?: string;
}

interface TwakeConfig {
  sso_url: string;
  client_id_interactive: string;
  client_id_background?: string;
  scopes: string[];
}

export class AuthService {
  private token: TokenResponse | null = null;
  private config: TwakeConfig | null = null;
  private tokenObtainedAt = 0;

  async configure(serverUrl: string): Promise<void> {
    const response = await fetch(
      `${serverUrl}/.well-known/twake/desktop-configuration`
    );
    this.config = await response.json();
  }

  async startInteractiveAuth(): Promise<TokenResponse> {
    if (!this.config) throw new Error('Not configured — call configure() first');

    // Generate PKCE codes
    const codeVerifier = randomBytes(32).toString('base64url');
    const codeChallenge = createHash('sha256')
      .update(codeVerifier)
      .digest('base64url');

    // Start local HTTP server for callback
    const { code, redirectUri } = await this.waitForAuthCallback(codeChallenge);

    // Exchange code for tokens
    const tokenResponse = await fetch(`${this.config.sso_url}/oauth2/token`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        grant_type: 'authorization_code',
        client_id: this.config.client_id_interactive,
        code,
        redirect_uri: redirectUri,
        code_verifier: codeVerifier,
      }),
    });

    this.token = await tokenResponse.json();
    this.tokenObtainedAt = Date.now();

    // Persist encrypted token
    this.saveToken();

    return this.token!;
  }

  async getToken(): Promise<{ access_token: string; expires_in: number } | null> {
    if (!this.token) {
      this.loadToken();
    }
    if (!this.token) return null;

    // Check expiry (with 60s buffer)
    const elapsed = (Date.now() - this.tokenObtainedAt) / 1000;
    if (elapsed >= this.token.expires_in - 60) {
      if (this.token.refresh_token) {
        await this.refreshToken();
      } else {
        return null; // Need re-auth
      }
    }

    return {
      access_token: this.token.access_token,
      expires_in: this.token.expires_in - Math.floor(elapsed),
    };
  }

  private async waitForAuthCallback(
    codeChallenge: string
  ): Promise<{ code: string; redirectUri: string }> {
    return new Promise((resolve, reject) => {
      const server = createServer((req, res) => {
        const url = new URL(req.url!, `http://127.0.0.1`);
        const code = url.searchParams.get('code');

        if (code) {
          res.writeHead(200, { 'Content-Type': 'text/html' });
          res.end('<html><body><h1>Authentication successful</h1><p>You can close this window.</p></body></html>');
          server.close();
          resolve({ code, redirectUri });
        } else {
          res.writeHead(400);
          res.end('Missing code');
        }
      });

      server.listen(0, '127.0.0.1', () => {
        const port = (server.address() as any).port;
        const redirectUri = `http://127.0.0.1:${port}/callback`;

        // Build auth URL
        const authUrl = new URL(`${this.config!.sso_url}/oauth2/auth`);
        authUrl.searchParams.set('client_id', this.config!.client_id_interactive);
        authUrl.searchParams.set('response_type', 'code');
        authUrl.searchParams.set('redirect_uri', redirectUri);
        authUrl.searchParams.set('scope', this.config!.scopes.join(' '));
        authUrl.searchParams.set('code_challenge', codeChallenge);
        authUrl.searchParams.set('code_challenge_method', 'S256');

        // Open auth window
        const authWin = new BrowserWindow({
          width: 500,
          height: 700,
          webPreferences: {
            sandbox: true,
            contextIsolation: true,
            nodeIntegration: false,
          },
        });

        authWin.loadURL(authUrl.toString());
        authWin.on('closed', () => {
          server.close();
          reject(new Error('Auth window closed'));
        });
      });
    });
  }

  private async refreshToken(): Promise<void> {
    if (!this.config || !this.token?.refresh_token) return;

    const response = await fetch(`${this.config.sso_url}/oauth2/token`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        grant_type: 'refresh_token',
        client_id: this.config.client_id_interactive,
        refresh_token: this.token.refresh_token,
      }),
    });

    this.token = await response.json();
    this.tokenObtainedAt = Date.now();
    this.saveToken();
  }

  private saveToken() {
    if (!this.token) return;
    const data = JSON.stringify({ token: this.token, obtainedAt: this.tokenObtainedAt });
    const encrypted = safeStorage.encryptString(data);
    // Write to app data dir
    const fs = require('fs');
    const path = require('path');
    const { app } = require('electron');
    const tokenPath = path.join(app.getPath('userData'), 'auth.enc');
    fs.writeFileSync(tokenPath, encrypted);
  }

  private loadToken() {
    try {
      const fs = require('fs');
      const path = require('path');
      const { app } = require('electron');
      const tokenPath = path.join(app.getPath('userData'), 'auth.enc');
      const encrypted = fs.readFileSync(tokenPath);
      const data = JSON.parse(safeStorage.decryptString(encrypted));
      this.token = data.token;
      this.tokenObtainedAt = data.obtainedAt;
    } catch {
      this.token = null;
    }
  }
}
```

---

## Security

### BrowserWindow Security Defaults

Every BrowserWindow is created with:

```typescript
webPreferences: {
  sandbox: true,           // Renderer in Chromium sandbox
  contextIsolation: true,  // Preload in isolated world
  nodeIntegration: false,  // No Node.js in renderer
  webSecurity: true,       // Same-origin policy enforced
}
```

### Content Security Policy

Applied via custom protocol handler response headers:

```
default-src 'self' twake:;
script-src 'self' twake:;
style-src 'self' twake: 'unsafe-inline';
connect-src https: wss: twake:;
img-src 'self' twake: https: data:;
font-src 'self' twake: https:;
```

### IPC Channel Whitelist

Only these channels are handled by the main process:

| Channel | Direction | Purpose |
|---------|-----------|---------|
| `twake:file:status` | renderer → main | Get file state |
| `twake:file:hydrate` | renderer → main | Download file |
| `twake:file:list` | renderer → main | List directory |
| `twake:auth:token` | renderer → main | Get current token |
| `twake:auth:start` | renderer → main | Start OIDC flow |
| `twake:event:*` | main → renderer | Push events from Rust |

### Navigation Restrictions

```typescript
// Only allow these protocol/origin combinations
win.webContents.on('will-navigate', (event, url) => {
  const parsed = new URL(url);
  if (parsed.protocol !== 'twake:' && parsed.protocol !== 'https:') {
    event.preventDefault();
  }
});
```

---

## Project Structure

```
electron-shell/
├── package.json
├── tsconfig.json
├── electron-builder.yml
├── src/
│   ├── main.ts               # Entry point
│   ├── preload.ts             # contextBridge (window.__twake)
│   ├── windows.ts             # BrowserWindow factory
│   ├── protocol.ts            # twake:// protocol handler
│   ├── ipc-bridge.ts          # Electron IPC ↔ Rust JSON-RPC
│   ├── sidecar.ts             # Rust process lifecycle
│   ├── auth.ts                # OIDC PKCE
│   └── tray.ts                # Tray icon + context menu
├── renderer/                  # Local SPA (served via twake://)
│   ├── index.html
│   ├── app.js
│   └── styles.css
└── resources/
    └── bin/                   # Rust binary (packaged)
        └── twake-sync
```

---

## Build System

### package.json

```json
{
  "name": "twake-desktop",
  "version": "0.1.0",
  "main": "dist/main.js",
  "scripts": {
    "dev": "electron-vite dev",
    "build": "tsc && esbuild src/main.ts --bundle --platform=node --outfile=dist/main.js --external:electron",
    "build:preload": "esbuild src/preload.ts --bundle --platform=node --outfile=dist/preload.js --external:electron",
    "package": "electron-builder",
    "start": "electron dist/main.js"
  },
  "devDependencies": {
    "electron": "^33.0.0",
    "electron-builder": "^25.0.0",
    "esbuild": "^0.24.0",
    "typescript": "^5.5.0"
  }
}
```

### electron-builder.yml

```yaml
appId: com.twake.desktop
productName: Twake Desktop
directories:
  output: release
files:
  - dist/**/*
  - renderer/**/*
  - "!node_modules/**/*"
extraResources:
  - from: ../sync-engine/target/release/twake-sync
    to: bin/twake-sync
asar: true
linux:
  target: [AppImage, deb]
  category: Office
mac:
  target: [dmg, zip]
win:
  target: [nsis]
```

### Build Commands

```bash
# Install dependencies
npm install

# Development
npm run dev

# Build for production
npm run build
npm run build:preload
npm run package

# Build Rust sidecar
cd ../sync-engine && cargo build --release
```

---

## Testing Strategy

### Unit Tests

- Preload bridge type validation
- IPC channel whitelist enforcement
- Protocol path traversal prevention
- Auth token expiry logic

### Integration Tests

- Window creation lifecycle
- contextBridge injection
- IPC round-trip (renderer → main → Rust mock → main → renderer)
- Custom protocol file serving

### E2E Tests

- Full auth flow (OIDC PKCE with mock SSO)
- SPA loads via `twake://bundle/`
- Bridge calls work end-to-end
- Event propagation from Rust to renderer

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Security misconfiguration** | High | Defaults are secure; security checklist in CI |
| **Memory usage** | Medium | Lazy windows, monitor with `process.memoryUsage()` |
| **Sidecar crash** | Medium | Auto-restart with exponential backoff |
| **Electron version churn** | Low | Use LTS releases, test before upgrade |
| **Bundle size** | Low | esbuild bundling, ASAR, exclude dev deps |

---

## Development Checklist (MVP)

### Day 1 -- Project Setup

- [ ] `npm init` + install Electron + TypeScript + esbuild
- [ ] `main.ts` with BrowserWindow creation
- [ ] `preload.ts` with contextBridge skeleton
- [ ] `protocol.ts` with `twake://` handler
- [ ] Local SPA (`renderer/index.html`) loads via `twake://bundle/`

### Day 2 -- Auth + Bridge

- [ ] `auth.ts` with OIDC PKCE flow
- [ ] `ipc-bridge.ts` with channel handlers
- [ ] `sidecar.ts` with Rust process spawn + socket connect
- [ ] `window.__twake.startAuth()` works end-to-end
- [ ] `window.__twake.getToken()` returns token

### Day 3 -- Integration

- [ ] Local SPA can authenticate and display token info
- [ ] Bridge calls reach Rust sidecar (or mock)
- [ ] Events from Rust propagate to SPA
- [ ] Tray icon with context menu
- [ ] Demo preparation

---

## References

- [STREAM_A_ELECTRON.md](../../../STREAM_A_ELECTRON.md) -- Implementation guide
- [INTERFACES.md](../../../INTERFACES.md) -- IPC contract
- [IPC Contract Design Spec](ipc-contract-design.md) -- JSON-RPC methods and events
- [ADR-0004](../../adr/ADR-0004-electron-migration.md) -- Electron migration decision
- [Electron Security Checklist](https://www.electronjs.org/docs/latest/tutorial/security)
- [contextBridge API](https://www.electronjs.org/docs/latest/api/context-bridge)
