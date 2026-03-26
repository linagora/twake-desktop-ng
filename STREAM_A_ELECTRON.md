# Stream A -- Electron Shell

**Responsable:** Dev 1
**Stack:** TypeScript, Electron, esbuild
**Objectif:** Shell desktop avec BrowserWindows, bridge `window.__twake`, auth OIDC

---

## Jour 1 -- Infrastructure Electron

### Matin (08:00 - 10:00) -- Setup projet

**Tache A1.1: Initialiser le projet**

```bash
mkdir electron-shell && cd electron-shell
npm init -y
npm install --save-dev electron typescript esbuild @types/node
npx tsc --init --strict --target es2022 --module commonjs --outDir dist
```

**Tache A1.2: Structure des fichiers**

```
electron-shell/
├── package.json
├── tsconfig.json
├── src/
│   ├── main.ts
│   ├── preload.ts
│   ├── windows.ts
│   ├── protocol.ts
│   ├── ipc-bridge.ts
│   ├── sidecar.ts
│   ├── auth.ts
│   └── tray.ts
└── renderer/
    ├── index.html
    ├── app.js
    └── styles.css
```

**Tache A1.3: Script de build**

```json
{
  "scripts": {
    "build:main": "esbuild src/main.ts --bundle --platform=node --outfile=dist/main.js --external:electron",
    "build:preload": "esbuild src/preload.ts --bundle --platform=node --outfile=dist/preload.js --external:electron",
    "build": "npm run build:main && npm run build:preload",
    "start": "npm run build && electron dist/main.js",
    "dev": "npm run build && electron --inspect dist/main.js"
  }
}
```

**Critere de succes:** `npm start` ouvre une fenetre Electron vide

---

### Matin (10:00 - 12:00) -- Protocole custom + SPA locale

**Tache A1.4: Protocole `twake://`**

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

    let filePath = decodeURIComponent(url.pathname);
    if (filePath === '/' || filePath === '') filePath = '/index.html';

    const bundleDir = path.resolve(path.join(__dirname, '..', 'renderer'));
    const resolved = path.resolve(path.join(bundleDir, filePath.replace(/^\//, '')));

    // Securite : pas de path traversal
    if (!resolved.startsWith(bundleDir)) {
      return new Response('Forbidden', { status: 403 });
    }

    return net.fetch(pathToFileURL(resolved).toString()).then(res => {
      const headers = new Headers(res.headers);
      headers.set('Content-Security-Policy',
        "default-src 'self' twake:; script-src 'self' twake:; style-src 'self' twake: 'unsafe-inline'; connect-src https: wss: twake:; img-src 'self' twake: https: data:");
      return new Response(res.body, { status: res.status, headers });
    });
  });
}
```

**Tache A1.5: SPA locale minimale**

```html
<!-- renderer/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Twake Desktop</title>
  <link rel="stylesheet" href="styles.css">
</head>
<body>
  <div id="app">
    <h1>Twake Desktop</h1>
    <div id="auth-section">
      <p id="auth-status">Not authenticated</p>
      <input type="text" id="server-url" placeholder="https://twake.company.com" />
      <button id="login-btn">Login with OIDC</button>
    </div>
    <div id="token-info" style="display:none">
      <p>Token: <code id="token-value">-</code></p>
      <p>Expires in: <span id="token-expiry">-</span>s</p>
    </div>
    <div id="bridge-test" style="display:none">
      <h2>Bridge Test</h2>
      <button id="test-status">Get File Status</button>
      <button id="test-list">List Files</button>
      <pre id="bridge-output"></pre>
    </div>
  </div>
  <script src="app.js"></script>
</body>
</html>
```

```javascript
// renderer/app.js
document.addEventListener('DOMContentLoaded', () => {
  const loginBtn = document.getElementById('login-btn');
  const authStatus = document.getElementById('auth-status');
  const tokenInfo = document.getElementById('token-info');
  const bridgeTest = document.getElementById('bridge-test');
  const output = document.getElementById('bridge-output');

  // Check if bridge is available
  if (window.__twake) {
    authStatus.textContent = 'Bridge loaded. Ready to authenticate.';
  } else {
    authStatus.textContent = 'ERROR: window.__twake not available';
    return;
  }

  // Login
  loginBtn.addEventListener('click', async () => {
    try {
      authStatus.textContent = 'Authenticating...';
      const token = await window.__twake.startAuth();
      authStatus.textContent = 'Authenticated!';
      document.getElementById('token-value').textContent =
        token.access_token.substring(0, 20) + '...';
      document.getElementById('token-expiry').textContent = token.expires_in;
      tokenInfo.style.display = 'block';
      bridgeTest.style.display = 'block';
    } catch (err) {
      authStatus.textContent = 'Auth failed: ' + err.message;
    }
  });

  // Bridge tests
  document.getElementById('test-status')?.addEventListener('click', async () => {
    try {
      const status = await window.__twake.getFileStatus('/test.txt');
      output.textContent = JSON.stringify(status, null, 2);
    } catch (err) {
      output.textContent = 'Error: ' + err.message;
    }
  });

  document.getElementById('test-list')?.addEventListener('click', async () => {
    try {
      const files = await window.__twake.listFiles('/', false);
      output.textContent = JSON.stringify(files, null, 2);
    } catch (err) {
      output.textContent = 'Error: ' + err.message;
    }
  });
});
```

**Critere de succes:** SPA se charge via `twake://bundle/index.html`

---

### Apres-midi (14:00 - 16:00) -- Preload + contextBridge

**Tache A1.6: Preload script**

```typescript
// src/preload.ts
import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('__twake', {
  // File operations
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

  // Auth
  getToken() {
    return ipcRenderer.invoke('twake:auth:token');
  },

  startAuth() {
    return ipcRenderer.invoke('twake:auth:start');
  },

  // Events
  on(event: string, callback: (...args: any[]) => void) {
    if (typeof event !== 'string') throw new TypeError('event must be string');
    const channel = `twake:event:${event}`;
    const listener = (_event: any, ...args: any[]) => callback(...args);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  },
});
```

**Tache A1.7: Main process avec fenetre**

```typescript
// src/main.ts
import { app, BrowserWindow } from 'electron';
import path from 'path';
import { registerTwakeProtocol } from './protocol';
import { setupIpcHandlers } from './ipc-bridge';

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) app.quit();

app.whenReady().then(() => {
  registerTwakeProtocol();
  setupIpcHandlers();

  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    show: false,
    webPreferences: {
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js'),
    },
  });

  win.once('ready-to-show', () => win.show());
  win.loadURL('twake://bundle/index.html');

  // Security: restrict navigation
  win.webContents.on('will-navigate', (event, url) => {
    const parsed = new URL(url);
    if (!['twake:', 'https:'].includes(parsed.protocol)) {
      event.preventDefault();
    }
  });

  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
```

**Critere de succes:** `window.__twake` accessible dans la SPA

---

### Apres-midi (16:00 - 18:00) -- IPC Bridge (mock)

**Tache A1.8: IPC handlers avec mock**

```typescript
// src/ipc-bridge.ts
import { ipcMain } from 'electron';

export function setupIpcHandlers() {
  // Mock handlers for Day 1 (no Rust sidecar yet)
  ipcMain.handle('twake:file:status', async (_event, path: string) => {
    console.log('[IPC] file.status:', path);
    return {
      path,
      state: 'ghost',
      size: 1024,
      modified: new Date().toISOString(),
    };
  });

  ipcMain.handle('twake:file:hydrate', async (_event, path: string) => {
    console.log('[IPC] file.hydrate:', path);
    return { success: true };
  });

  ipcMain.handle('twake:file:list', async (_event, path: string, recursive: boolean) => {
    console.log('[IPC] file.list:', path, recursive);
    return [
      { id: '1', path: '/documents/test.txt', state: 'ghost', size: 1024, is_dir: false },
      { id: '2', path: '/documents/photo.jpg', state: 'ghost', size: 102400, is_dir: false },
      { id: '3', path: '/shared', state: 'ghost', size: 0, is_dir: true },
    ];
  });

  ipcMain.handle('twake:auth:token', async () => {
    console.log('[IPC] auth.token');
    return null; // Not authenticated yet
  });

  ipcMain.handle('twake:auth:start', async () => {
    console.log('[IPC] auth.start');
    // Mock auth for Day 1
    return {
      access_token: 'mock_token_' + Date.now(),
      expires_in: 3600,
    };
  });
}
```

**Critere de succes:** SPA peut appeler `window.__twake.getFileStatus()` et recevoir une reponse mock

---

## Jour 2 -- Auth OIDC + Sidecar

### Matin (08:00 - 10:00) -- Auth OIDC reelle

**Tache A2.1: Service d'authentification**

```typescript
// src/auth.ts
import { BrowserWindow, safeStorage } from 'electron';
import { createServer } from 'http';
import { randomBytes, createHash } from 'crypto';
import path from 'path';
import fs from 'fs';
import { app } from 'electron';

export class AuthService {
  private token: any = null;
  private tokenObtainedAt = 0;
  private config: any = null;

  async configure(serverUrl: string) {
    const res = await fetch(`${serverUrl}/.well-known/twake/desktop-configuration`);
    this.config = await res.json();
  }

  async startInteractiveAuth(): Promise<any> {
    if (!this.config) throw new Error('Not configured');

    const codeVerifier = randomBytes(32).toString('base64url');
    const codeChallenge = createHash('sha256').update(codeVerifier).digest('base64url');

    const { code, redirectUri } = await this.openAuthWindow(codeChallenge);

    const tokenRes = await fetch(`${this.config.sso_url}/oauth2/token`, {
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

    this.token = await tokenRes.json();
    this.tokenObtainedAt = Date.now();
    this.saveToken();
    return this.token;
  }

  getToken() {
    if (!this.token) this.loadToken();
    if (!this.token) return null;

    const elapsed = (Date.now() - this.tokenObtainedAt) / 1000;
    if (elapsed >= this.token.expires_in - 60) return null;

    return {
      access_token: this.token.access_token,
      expires_in: this.token.expires_in - Math.floor(elapsed),
    };
  }

  private openAuthWindow(codeChallenge: string): Promise<{ code: string; redirectUri: string }> {
    return new Promise((resolve, reject) => {
      const server = createServer((req, res) => {
        const url = new URL(req.url!, 'http://127.0.0.1');
        const code = url.searchParams.get('code');
        if (code) {
          res.writeHead(200, { 'Content-Type': 'text/html' });
          res.end('<h1>OK</h1><p>You can close this window.</p>');
          server.close();
          resolve({ code, redirectUri: `http://127.0.0.1:${(server.address() as any).port}/callback` });
        }
      });

      server.listen(0, '127.0.0.1', () => {
        const port = (server.address() as any).port;
        const authUrl = new URL(`${this.config.sso_url}/oauth2/auth`);
        authUrl.searchParams.set('client_id', this.config.client_id_interactive);
        authUrl.searchParams.set('response_type', 'code');
        authUrl.searchParams.set('redirect_uri', `http://127.0.0.1:${port}/callback`);
        authUrl.searchParams.set('scope', (this.config.scopes || []).join(' '));
        authUrl.searchParams.set('code_challenge', codeChallenge);
        authUrl.searchParams.set('code_challenge_method', 'S256');

        const authWin = new BrowserWindow({ width: 500, height: 700,
          webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false }
        });
        authWin.loadURL(authUrl.toString());
        authWin.on('closed', () => { server.close(); reject(new Error('Auth cancelled')); });
      });
    });
  }

  private saveToken() {
    if (!this.token) return;
    const data = JSON.stringify({ token: this.token, obtainedAt: this.tokenObtainedAt });
    const encrypted = safeStorage.encryptString(data);
    fs.writeFileSync(path.join(app.getPath('userData'), 'auth.enc'), encrypted);
  }

  private loadToken() {
    try {
      const encrypted = fs.readFileSync(path.join(app.getPath('userData'), 'auth.enc'));
      const data = JSON.parse(safeStorage.decryptString(encrypted));
      this.token = data.token;
      this.tokenObtainedAt = data.obtainedAt;
    } catch { this.token = null; }
  }
}
```

**Tache A2.2: Brancher auth dans IPC**

```typescript
// src/ipc-bridge.ts (updated)
import { ipcMain, BrowserWindow } from 'electron';
import { AuthService } from './auth';
import { SidecarManager } from './sidecar';

export function setupIpcHandlers(auth?: AuthService, sidecar?: SidecarManager) {
  ipcMain.handle('twake:auth:start', async () => {
    if (!auth) return { error: 'Auth not configured' };
    return auth.startInteractiveAuth();
  });

  ipcMain.handle('twake:auth:token', async () => {
    if (!auth) return null;
    return auth.getToken();
  });

  // File ops: delegate to sidecar if available, mock otherwise
  ipcMain.handle('twake:file:status', async (_event, filePath: string) => {
    if (sidecar) return sidecar.call('file.status', { path: filePath });
    return { path: filePath, state: 'ghost', size: 0, modified: new Date().toISOString() };
  });

  ipcMain.handle('twake:file:hydrate', async (_event, filePath: string) => {
    if (sidecar) return sidecar.call('file.hydrate', { path: filePath });
    return { success: true };
  });

  ipcMain.handle('twake:file:list', async (_event, filePath: string, recursive: boolean) => {
    if (sidecar) return sidecar.call('file.list', { path: filePath, recursive });
    return [];
  });

  // Forward sidecar events to renderers
  if (sidecar) {
    sidecar.onEvent((event, data) => {
      for (const win of BrowserWindow.getAllWindows()) {
        win.webContents.send(`twake:event:${event}`, data);
      }
    });
  }
}
```

**Critere de succes:** OIDC PKCE fonctionne avec un vrai serveur SSO (ou mock)

---

### Matin (10:00 - 12:00) -- Sidecar Rust

**Tache A2.3: Sidecar manager**

```typescript
// src/sidecar.ts
import { spawn, ChildProcess } from 'child_process';
import { createConnection, Socket } from 'net';
import { app } from 'electron';
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

  async start() {
    const bin = process.platform === 'win32' ? 'twake-sync.exe' : 'twake-sync';
    const binPath = app.isPackaged
      ? path.join(process.resourcesPath, 'bin', bin)
      : path.join(__dirname, '..', '..', 'sync-engine', 'target', 'release', bin);

    this.process = spawn(binPath, ['--socket', this.socketPath], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    this.process.on('exit', (code) => {
      if (code !== 0) setTimeout(() => this.start(), 2000);
    });

    await this.waitForSocket();
    await this.connect();
  }

  async call(method: string, params: Record<string, unknown>) {
    const id = ++this.requestId;
    const msg = JSON.stringify({ jsonrpc: '2.0', method, params, id }) + '\n';

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket!.write(msg);
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`Timeout: ${method}`));
        }
      }, 30_000);
    });
  }

  onEvent(listener: (event: string, data: any) => void) {
    this.eventListeners.push(listener);
  }

  stop() { this.socket?.destroy(); this.process?.kill(); }

  private connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.socket = createConnection(this.socketPath);
      let buffer = '';
      this.socket.on('data', (chunk) => {
        buffer += chunk.toString();
        const lines = buffer.split('\n');
        buffer = lines.pop()!;
        for (const line of lines) {
          if (!line.trim()) continue;
          try { this.handleMessage(JSON.parse(line)); } catch {}
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
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    } else if (msg.params) {
      const data = msg.params.result || msg.params;
      for (const l of this.eventListeners) l(data.type || msg.method, data);
    }
  }

  private waitForSocket(): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        const s = createConnection(this.socketPath);
        s.once('connect', () => { s.destroy(); resolve(); });
        s.once('error', () => setTimeout(check, 100));
      };
      setTimeout(check, 200);
    });
  }
}
```

**Critere de succes:** Sidecar Rust demarre, socket connecte

---

### Apres-midi (14:00 - 18:00) -- Integration + Demo

**Tache A2.4: Tray icon**

```typescript
// src/tray.ts
import { Tray, Menu, nativeImage, app } from 'electron';
import path from 'path';

export function createTray(): Tray {
  const icon = nativeImage.createFromPath(
    path.join(__dirname, '..', 'resources', 'icon.png')
  );
  const tray = new Tray(icon);

  tray.setContextMenu(Menu.buildFromTemplate([
    { label: 'Open Twake', click: () => { /* focus main window */ } },
    { type: 'separator' },
    { label: 'Quit', click: () => app.quit() },
  ]));

  tray.setToolTip('Twake Desktop');
  return tray;
}
```

**Tache A2.5: Main.ts complet**

```typescript
// src/main.ts (final)
import { app, BrowserWindow } from 'electron';
import path from 'path';
import { registerTwakeProtocol } from './protocol';
import { setupIpcHandlers } from './ipc-bridge';
import { AuthService } from './auth';
import { SidecarManager } from './sidecar';
import { createTray } from './tray';

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) app.quit();

app.whenReady().then(async () => {
  registerTwakeProtocol();

  const auth = new AuthService();
  let sidecar: SidecarManager | undefined;

  try {
    sidecar = new SidecarManager();
    await sidecar.start();
  } catch (err) {
    console.warn('Sidecar not available, running with mocks:', err);
  }

  setupIpcHandlers(auth, sidecar);
  createTray();

  const win = new BrowserWindow({
    width: 1200, height: 800, show: false,
    webPreferences: {
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js'),
    },
  });

  win.once('ready-to-show', () => win.show());
  win.loadURL('twake://bundle/index.html');
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
```

**Tache A2.6: Demo script**

```bash
#!/bin/bash
# demo.sh

echo "1. Building Electron shell..."
cd electron-shell
npm run build

echo "2. Starting Rust sidecar (if available)..."
cd ../sync-engine
cargo build --release 2>/dev/null && echo "Rust sidecar built" || echo "Rust sidecar not available (mock mode)"

echo "3. Launching Twake Desktop..."
cd ../electron-shell
npm start
```

**Critere de succes:** Demo 5 minutes : login OIDC, token visible, bridge calls fonctionnels

---

## Build Commands

```bash
# Install
npm install

# Development
npm run dev

# Production build
npm run build
npm run package

# Debug
npm start -- --inspect
```

## Dependencies

```json
{
  "devDependencies": {
    "electron": "^33.0.0",
    "typescript": "^5.5.0",
    "esbuild": "^0.24.0",
    "electron-builder": "^25.0.0"
  }
}
```

Aucune dependance runtime npm -- on utilise les APIs built-in d'Electron (`safeStorage`, `net.fetch`, `Notification`, `Tray`, `Menu`).

## Known Issues

- **Sidecar path:** En dev, cherche dans `sync-engine/target/release/`. En prod, dans `resources/bin/`.
- **Auth flow:** Necessite un vrai serveur SSO pour le flow complet. Mock disponible pour dev.
- **Hot reload:** Pas de hot reload pour le main process (restart requis). Le renderer peut etre recharge avec Ctrl+R.
