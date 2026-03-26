# Shell-CEF RPC — Protocol Specification

**Date:** 2026-03-26
**Component:** Shell-CEF (Rust + CEF)
**Status:** Implemented

---

## Overview

The Shell-CEF exposes an RPC interface over a **Unix socket**, allowing external clients (sync engine, CLI tools, tests) to control the embedded browser. The protocol is intentionally simple: one JSON request, one JSON response, no streaming.

This protocol is **separate** from the CEF↔SyncEngine IPC contract (JSON-RPC 2.0 on `/tmp/twake-ipc.sock`). Here, the Shell-CEF is the **server** and exposes the embedded browser's capabilities (page rendering, OIDC flows, etc.).

---

## Transport

### Unix Socket

- **Default path:** `/tmp/twake-shell-cef.sock`
- **Override:**
  - Environment variable: `TWAKE_SHELL_CEF_SOCK`
  - CLI option: `--sock /path/to/socket`
  - Priority: CLI > env > default
- **Permissions:** `0600` (owner-only access)
- **Type:** `AF_UNIX`, `SOCK_STREAM`
- **Encoding:** UTF-8
- **Lifecycle:** The socket file is created on shell startup and removed on shutdown (cleanup on exit + stale socket cleanup on startup)

### Framing

Each message (request or response) is a JSON object terminated by a **newline** character (`\n`). This enables simple framing compatible with standard tools (`socat`, `nc`, scripts).

```
CLIENT → {"action":"navigate","params":{"url":"https://example.com"}}\n
SERVER → {"status":"ok","data":{...},"meta":{...}}\n
```

One request/response exchange per connection (one-shot mode). The client connects, sends a request, receives the response, and the connection is closed. This simplifies the protocol and avoids multiplexing issues.

> **Note:** If persistent connections are needed later (e.g., push events), the protocol can evolve to a "keep-alive" mode with an `id` field for request/response correlation.

---

## Request Format

```json
{
  "action": "<action_name>",
  "params": { ... }
}
```

| Field    | Type   | Required | Description                        |
|----------|--------|----------|------------------------------------|
| `action` | string | yes      | Name of the action to execute      |
| `params` | object | no       | Action-specific parameters         |

---

## Response Format

### Success

```json
{
  "status": "ok",
  "data": { ... },
  "meta": {
    "duration_ms": 142,
    "timestamp": "2026-03-26T10:30:00Z"
  }
}
```

### Error

```json
{
  "status": "error",
  "error": {
    "code": "<ERROR_CODE>",
    "message": "Human-readable error description"
  },
  "meta": {
    "duration_ms": 3,
    "timestamp": "2026-03-26T10:30:00Z"
  }
}
```

| Field              | Type   | Required | Description                             |
|--------------------|--------|----------|-----------------------------------------|
| `status`           | string | yes      | `"ok"` or `"error"`                     |
| `data`             | object | if ok    | Response data (action-specific)         |
| `error`            | object | if error | Error details                           |
| `error.code`       | string | yes      | Machine-readable error code             |
| `error.message`    | string | yes      | Human-readable message                  |
| `meta`             | object | yes      | Response metadata                       |
| `meta.duration_ms` | u64    | yes      | Execution time in milliseconds          |
| `meta.timestamp`   | string | yes      | ISO 8601 response timestamp             |

---

## Actions

### `navigate` — Load a page and return its content

Loads a URL in the embedded browser and returns the resulting HTML content (after JavaScript execution).

**Request:**

```json
{
  "action": "navigate",
  "params": {
    "url": "https://example.com",
    "wait_until": "load",
    "timeout_ms": 30000
  }
}
```

| Parameter    | Type   | Required | Default  | Description                              |
|--------------|--------|----------|----------|------------------------------------------|
| `url`        | string | yes      | —        | URL to load                              |
| `wait_until` | string | no       | `"load"` | Event to wait for: `"load"` or `"domready"` |
| `timeout_ms` | u64    | no       | `30000`  | Timeout in ms before giving up           |

**Response (success):**

```json
{
  "status": "ok",
  "data": {
    "url": "https://example.com",
    "final_url": "https://example.com/",
    "title": "Example Domain",
    "status_code": 200,
    "body": "<!doctype html>..."
  },
  "meta": {
    "duration_ms": 842,
    "timestamp": "2026-03-26T10:30:00Z"
  }
}
```

| Field         | Type   | Description                                |
|---------------|--------|--------------------------------------------|
| `url`         | string | Requested URL (as provided)                |
| `final_url`   | string | Final URL after redirects                  |
| `title`       | string | Page title (`<title>`)                     |
| `status_code` | u16    | HTTP response status code                  |
| `body`        | string | HTML content of the page (DOM after JS)    |

**Response (error):**

```json
{
  "status": "error",
  "error": {
    "code": "NAVIGATE_TIMEOUT",
    "message": "Navigation timed out after 30000ms"
  },
  "meta": {
    "duration_ms": 30001,
    "timestamp": "2026-03-26T10:30:30Z"
  }
}
```

---

## Error Codes

| Code               | Description                                      |
|--------------------|--------------------------------------------------|
| `UNKNOWN_ACTION`   | The requested action does not exist              |
| `INVALID_PARAMS`   | Missing or invalid parameters                    |
| `INVALID_JSON`     | The request is not valid JSON                    |
| `NAVIGATE_TIMEOUT` | Page load timed out                              |
| `NAVIGATE_FAILED`  | Navigation failed (DNS, network, certificate)    |
| `INTERNAL_ERROR`   | Internal shell-CEF error                         |

---

### `auth.oidc_start` — OIDC PKCE Authentication Flow

Starts an OIDC PKCE authorization code flow, opens the browser for user login, and returns tokens upon success.

**Request:**

```json
{
  "action": "auth.oidc_start",
  "params": {
    "issuer": "https://sso.linagora.com",
    "client_id": "tcalendar",
    "redirect_uri": "http://localhost:5000/callback",
    "pkce": true,
    "scopes": ["openid", "profile", "email"]
  }
}
```

| Parameter       | Type    | Required | Default                           | Description                                    |
|-----------------|---------|----------|-----------------------------------|------------------------------------------------|
| `issuer`        | string  | yes      | —                                 | OIDC issuer URL                                |
| `client_id`     | string  | yes      | —                                 | OAuth client ID                               |
| `redirect_uri`  | string  | no       | `http://localhost:5000/callback`  | Callback URL                                  |
| `pkce`          | boolean | no       | `false`                           | Enable PKCE flow                              |
| `scopes`        | array   | no       | `["openid","profile","email"]`   | OAuth scopes                                  |

**Response (success):**

```json
{
  "status": "ok",
  "data": {
    "access_token": "eyJ...",
    "refresh_token": "eyJ...",
    "id_token": "eyJ...",
    "token_type": "Bearer",
    "expires_in": 3600
  },
  "meta": {
    "duration_ms": 12500,
    "timestamp": "2026-03-26T10:30:12Z"
  }
}
```

**Response (error):**

```json
{
  "status": "error",
  "error": {
    "code": "INTERNAL_ERROR",
    "message": "Failed to open browser: ..."
  },
  "meta": {
    "duration_ms": 123,
    "timestamp": "2026-03-26T10:30:00Z"
  }
}
```

---

## Future Actions (not implemented, reserved)

These actions illustrate the protocol's extensibility. They are not part of the MVP.

| Action              | Description                                            |
|---------------------|--------------------------------------------------------|
| `auth.oidc_start`   | Start an OIDC PKCE flow (issuer + client_id → tokens) |
| `auth.oidc_status`  | Check the status of an ongoing OIDC flow               |
| `page.screenshot`   | Capture a screenshot of the current page               |
| `page.evaluate`     | Execute arbitrary JavaScript in the page               |
| `page.cookies`      | Read/write browser cookies                             |
| `shell.status`      | Get shell status (version, uptime, memory)             |
| `shell.quit`        | Gracefully shut down the shell-CEF                     |

---

## Usage with Standard Tools

### socat

```bash
echo '{"action":"navigate","params":{"url":"https://example.com"}}' | \
  socat - UNIX-CONNECT:/tmp/twake-shell-cef.sock
```

### Shell script (curl-like)

```bash
#!/bin/bash
# twake-get: fetch a URL's content via Shell-CEF
URL="${1:?Usage: twake-get <url>}"
SOCK="${TWAKE_SHELL_CEF_SOCK:-/tmp/twake-shell-cef.sock}"

echo "{\"action\":\"navigate\",\"params\":{\"url\":\"$URL\"}}" | \
  socat - UNIX-CONNECT:"$SOCK" | jq .
```

### Integration test

```bash
# Verify that the shell is running
echo '{"action":"navigate","params":{"url":"https://httpbin.org/get"}}' | \
  socat - UNIX-CONNECT:/tmp/twake-shell-cef.sock | \
  jq -e '.status == "ok"'
```

---

## Shell-CEF CLI Configuration

```
twake-shell-cef [OPTIONS]

Options:
  --sock <PATH>        Unix socket path
                       [env: TWAKE_SHELL_CEF_SOCK]
                       [default: /tmp/twake-shell-cef.sock]

  --log-level <LEVEL>  Log level (error, warn, info, debug, trace)
                       [env: TWAKE_LOG_LEVEL]
                       [default: info]

  --help               Show help
  --version            Show version
```

---

## Security Considerations

- **Socket permissions:** `0600` — only the owning user can connect
- **No authentication:** In MVP, security relies solely on filesystem permissions. A token mechanism can be added if needed.
- **No arbitrary execution:** The `navigate` action only loads URLs. The `page.evaluate` action (future) must be restricted to trusted domains.
- **Stale socket:** On startup, if the socket file already exists, the shell checks that it is not in use by another process before removing it.

---

## Sequence Diagram

```
Client                         Shell-CEF                    CEF Browser
  │                               │                             │
  │─── connect() ────────────────►│                             │
  │─── {"action":"navigate",...}──►│                             │
  │                               │── LoadURL(url) ────────────►│
  │                               │                             │
  │                               │◄── OnLoadEnd(status) ───────│
  │                               │◄── GetSource(html) ─────────│
  │                               │                             │
  │◄── {"status":"ok","data":{…}} │                             │
  │─── close() ──────────────────►│                             │
```

---

## References

- [IPC Contract Design](ipc-contract-design.md) — CEF↔SyncEngine IPC contract (separate protocol)
- [CEF Shell Design](cef-shell-design.md) — CEF shell architecture
- [INTERFACES.md](../../INTERFACES.md) — Global interface contracts
- [ADR-0003](../adr/ADR-0003-two-process-architecture.md) — Two-process architecture
- [tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs) — Rust bindings for CEF
