# CEF Shell Design Spec

**Date:** 2026-03-25  
**Component:** CEF Shell (C++)  
**Stream:** A - CEF Shell  
**Status:** Draft

---

## Overview

The CEF Shell provides the desktop application framework that hosts Twake web applications in native windows. It manages multiple WebViews, injects a JavaScript bridge, and communicates with the Rust sync engine via IPC.

**Key principle:** Minimal C++ code (500-1000 lines), security through isolation.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    CEF Shell (C++)                      │
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │   Drive     │  │    Mail     │  │  Calendar   │     │
│  │  WebView    │  │  WebView    │  │  WebView    │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
│         │                  │                  │         │
│         └──────────────────┼──────────────────┘         │
│                            ▼                            │
│  ┌─────────────────────────────────────────────────┐   │
│  │              CEF Components                      │   │
│  │  - Window management (native OS APIs)           │   │
│  │  - Tray icon (Win32/NSStatusItem/libnotify)    │   │
│  │  - JS Bridge injection (window.__twake)        │   │
│  │  - IPC client (JSON-RPC to Rust)               │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

---

## Components

### 1. Window Management

**Responsibilities:**
- Create/Close/Minimize/Maximize windows
- Native window decorations
- Single instance lock
- Window persistence (position, size)

**Implementation:**

```cpp
class WindowManager {
public:
    void createWindow(const std::string& url, WindowOptions options);
    void closeWindow(BrowserId id);
    void minimizeWindow(BrowserId id);
    void restoreWindow(BrowserId id);

private:
    std::map<BrowserId, CefRefPtr<CefBrowser>> browsers_;
    WindowPersistence persistence_;
};
```

**CEF Configuration:**

```cpp
CefSettings settings;
settings.multi_threaded_message_loop = true;
settings.external_message_pump = false;
settings.windowless_rendering_enabled = false;
settings.no_sandbox = true;  // MVP only

// Force one renderer process per origin
// (default behavior, no extra config needed)
```

### 2. JavaScript Bridge

**Responsibilities:**
- Inject `window.__twake` into Twake WebViews
- Filter by domain (only on Twake domains)
- Expose IPC methods to JavaScript
- Forward events from Rust to JS

**Implementation:**

```cpp
class JsBridge {
public:
    void injectBridge(CefRefPtr<CefBrowser> browser);

private:
    bool isTwakeDomain(const std::string& url);
    void registerMethods(CefRefPtr<CefV8Context> context);
    void dispatchEvent(CefRefPtr<CefBrowser> browser,
                       const std::string& event,
                       const std::string& data);
};

void JsBridge::injectBridge(CefRefPtr<CefBrowser> browser) {
    // Called in OnContextCreated
    CefRefPtr<CefV8Value> twake = CefV8Value::CreateObject();

    twake->SetValue("getFileStatus",
        CefV8Handler::Create([this](const CefV8ValueList& args) {
            std::string path = args[0]->GetStringValue();
            auto status = ipc_client_->getFileStatus(path);
            return CefV8Value::CreateString(status.toJSON());
        }),
        V8_PROPERTY_ATTRIBUTE_READONLY);

    twake->SetValue("hydrateFile",
        CefV8Handler::Create([this](const CefV8ValueList& args) {
            std::string path = args[0]->GetStringValue();
            bool success = ipc_client_->hydrateFile(path);
            return CefV8Value::CreateBool(success);
        }),
        V8_PROPERTY_ATTRIBUTE_READONLY);

    context->GetGlobal()->SetValue("__twake", twake,
                                   V8_PROPERTY_ATTRIBUTE_NONE);
}
```

**JavaScript API:**

```javascript
// Synchronous
const status = window.__twake.getFileStatus('/documents/test.txt');

// Asynchronous
await window.__twake.hydrateFile('/documents/test.txt');

// Event subscription
window.__twake.on('file_changed', (data) => {
    console.log('File changed:', data.path, data.state);
});
```

### 3. IPC Client

**Responsibilities:**
- Connect to Rust sync engine
- Call JSON-RPC methods
- Handle disconnections and retries
- Forward events from Rust

**Implementation:**

```cpp
class IpcClient {
public:
    bool connect(const std::string& socket_path);
    void disconnect();

    FileStatus getFileStatus(const std::string& path);
    bool hydrateFile(const std::string& path);
    std::vector<FileNode> listFiles(const std::string& path, bool recursive);

    void subscribeEvents(EventCallback callback);

private:
    int socket_fd_;
    bool connected_;
    EventCallback event_callback_;

    bool reconnect();
    void sendRequest(const JsonRpcRequest& request);
    JsonRpcResponse receiveResponse();
};
```

**Connection Management:**

```cpp
FileStatus IpcClient::getFileStatus(const std::string& path) {
    if (!connected_) {
        LOG(WARNING) << "IPC not connected, attempting reconnect";
        if (!reconnect()) {
            throw IpcError("Connection failed");
        }
    }

    try {
        return sendRequest("file.status", {{"path", path}});
    } catch (...) {
        LOG(ERROR) << "IPC request failed, marking disconnected";
        connected_ = false;
        throw;
    }
}
```

### 4. Event Handler

**Responsibilities:**
- Receive events from Rust via IPC
- Dispatch to appropriate WebViews
- Handle event subscriptions

**Implementation:**

```cpp
void EventDispatcher::onEvent(const std::string& event, const std::string& data) {
    // Dispatch to all subscribed browsers
    for (auto& [id, browser] : browsers_) {
        CefRefPtr<CefProcessMessage> message =
            CefProcessMessage::Create("TWAKE_EVENT");

        CefArgumentList& args = message->GetArgumentList();
        args.SetString(0, event);
        args.SetString(1, data);

        browser->GetMainFrame()->SendProcessMessage(
            CefProcessId::PID_RENDERER, message);
    }
}

void JsBridge::OnProcessMessageReceived(
    CefRefPtr<CefBrowser> browser,
    CefRefPtr<CefFrame> frame,
    CefProcessId source_process,
    CefRefPtr<CefProcessMessage> message) {

    if (message->GetName() == "TWAKE_EVENT") {
        std::string event = message->GetArgumentList()->GetString(0).ToString();
        std::string data = message->GetArgumentList()->GetString(1).ToString();

        // Dispatch to JS
        std::string js =
            "window.__twake._dispatch('" + event + "', " + data + ");";

        frame->ExecuteJavaScript(js, frame->GetURL(), 0);
    }
}
```

---

## Security

### Domain Filtering

```cpp
bool JsBridge::isTwakeDomain(const std::string& url) {
    // Only inject bridge on trusted Twake domains
    return url.find("app1.twake.app") != std::string::npos ||
           url.find("app2.twake.app") != std::string::npos ||
           url.find("twake.company.com") != std::string::npos;
}
```

### Renderer Process Isolation

CEF automatically isolates renderer processes by origin:

```
app1.twake.app (window 1) → Renderer Process A
app1.twake.app (window 2) → Renderer Process A (same origin)
app2.twake.app (window 3) → Renderer Process B
```

**Benefit:** If one WebView crashes, others remain unaffected.

### Crash Recovery

```cpp
void WindowManager::OnProcessCrashed(CefRefPtr<CefBrowser> browser) {
    LOG(ERROR) << "Renderer process crashed, reloading window";

    // Reload the browser
    browser->GetMainFrame()->Reload();

    // Optionally notify user
    event_bus_->publish(BrowserCrashedEvent{browser->GetIdentifier()});
}
```

---

## Native Integration

### Tray Icon

```cpp
class TrayIcon {
public:
    void create();
    void setTooltip(const std::string& tooltip);
    void setContextMenu(TrayMenu menu);

private:
    // Platform-specific implementations
    void createWin32();      // Windows
    void createCocoa();      // macOS
    void createGtk();        // Linux
};
```

### Notifications

```cpp
class Notifications {
public:
    void show(const std::string& title, const std::string& body);

private:
    void showWinRT(const std::string& title, const std::string& body);
    void showNSUserNotification(const std::string& title, const std::string& body);
    void showLibnotify(const std::string& title, const std::string& body);
};
```

---

## Build System

### CMakeLists.txt

```cmake
cmake_minimum_required(VERSION 3.16)
project(twake-cef)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# CEF directory
set(CEF_ROOT "${CMAKE_SOURCE_DIR}/cef")
find_package(CEF REQUIRED)

# Main executable
add_executable(twake_app
    src/main.cpp
    src/app/browser_app.cpp
    src/app/render_app.cpp
    src/browser/window_manager.cpp
    src/browser/js_bridge.cpp
    src/browser/tray_icon.cpp
    src/ipc/ipc_client.cpp
)

target_link_libraries(twake_app
    ${CEF_LIBRARIES}
    pthread
)

include_directories(${CEF_INCLUDE_PATH})
```

### Build Commands

```bash
# Download CEF binaries
wget https://cef-builds.spotifycdn.com/cef_binary_122.1.11+g5c8b4c2+chromium-122.0.6261.111_linux64.tar.bz2
tar -xjf cef_binary_*.tar.bz2

# Build
mkdir build && cd build
cmake ..
make -j$(nproc)

# Run
./twake_app
```

---

## Dependencies

```cmake
# Required
- CEF 122.x (prebuilt binaries)
- nlohmann/json (header-only)
- pthread

# Optional (platform-specific)
- libappindicator (Linux tray icon)
- WinRT (Windows notifications)
- NSUserNotification (macOS notifications)
- libnotify (Linux notifications)
```

---

## Testing Strategy

### Unit Tests

- Domain filtering logic
- JSON serialization/deserialization
- IPC message formatting

### Integration Tests

- Window creation and lifecycle
- JS bridge injection
- IPC method calls
- Event dispatch

### E2E Tests

- Full flow: WebView → JS bridge → IPC → Rust → response
- Event propagation: Rust → IPC → C++ → JS
- Crash recovery
- Multi-window scenarios

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **CEF build complexity** | High | Use prebuilt binaries, document setup |
| **Sandbox disabled** | Medium | Enable in production, MVP OK |
| **IPC disconnects** | Medium | Reconnect logic, retry with backoff |
| **Memory usage** | Medium | Monitor, limit number of windows |
| **Renderer crash** | Low | Auto-reload, isolate by origin |

---

## Development Checklist

### Day 1

- [ ] Download CEF binaries
- [ ] Setup CMake build
- [ ] CefInitialize + message loop
- [ ] Browser creation (1 window)
- [ ] IPC client skeleton

### Day 2

- [ ] JS bridge injection
- [ ] Domain filtering
- [ ] IPC method calls (getFileStatus, hydrateFile)
- [ ] Event dispatch from Rust
- [ ] End-to-end test

### Day 3-4

- [ ] Window management (close, minimize, maximize)
- [ ] Tray icon (platform-specific)
- [ ] Notifications (platform-specific)
- [ ] Crash recovery
- [ ] Error handling

### Day 5

- [ ] Integration testing
- [ ] Performance optimization
- [ ] Documentation
- [ ] Demo preparation

---

## References

- [STREAM_A_CEF.md](../../STREAM_A_CEF.md) - Detailed implementation guide
- [INTERFACES.md](../../INTERFACES.md) - IPC contract
- [IPC Contract Design Spec](../ipc-contract-design.md) - IPC methods and events
- [CEF Documentation](https://bitbucket.org/chromiumembedded/cef)
- [jsonrpsee C++ client](https://github.com/paritytech/jsonrpsee)
