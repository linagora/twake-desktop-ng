# Twake Desktop NG - Project Notes

**Source:** https://benji-notes.mycozy.cloud/public/?id=019d254d-792b-7fe2-97df-7483a28f4087

**Date:** 2026-03-25

**Author:** Ben

---

## Project Objective

Develop a new desktop client for Twake.

This client has multiple roles aimed at providing an enriched desktop experience for Twake web services.

---

## Key Features

### Advanced Synchronization
Exposure of the synchronized file system via a virtual file system (VFS).

### Web Services on Desktop
Opening Twake web apps in native windows, like a browser without an address bar.

### Offline and Cache Management
- Offline support with configurable cache (variable strategies) for web apps
- Virtual exposure of all files without systematic binary downloads

### Unified Notification Center
Centralized notifications on the desktop for all Twake services (mail, chat, calendar, video).

### Embedded File Editing
- Integration of editors like OnlyOffice to open VFS files
- Collaborative mode with OnlyOffice server (online); local saving offline
- Avoids using native Word, Excel, PowerPoint files

---

## Technical Advantages

| Aspect | Benefit |
|--------|---------|
| Virtual System | Better conflict management; simplified file events; optimized cache |
| Offline Mode | Continuous usage without network; deferred sync |
| Editor Integration | Native productivity; smooth collaboration |
| Notifications | Unified experience, without multiple trays |

---

## Proposed Technologies

**Main Framework:** Electron (simple for webviews) or Rust-based alternative (e.g., Tauri/Wry for lightweight).

**Criteria:** Webview integration ease; cross-platform support; low footprint.

*Note: This structure is ready for a specification or dev brief. Need additions (e.g., priorities, mockups)?*

---

## MVP

1. Can authenticate
2. Can synchronize a local directory with the remote one
3. Can open a web app in a local window (online for now)

---

## First Steps

**Technology and language choice:**
- Go / Rust / Java
- Packaging: ...

---

*Provisional document generated on 2026-03-25*
