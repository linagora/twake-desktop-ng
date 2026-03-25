# Stream A — CEF Shell

**Responsable:** Dev 1  
**Stack:** C++, CMake, CEF  
**Objectif:** Shell desktop avec WebViews Twake

---

## Jour 1 — Infrastructure CEF

### Matin (08:00 - 10:00) — Setup CEF

**Tâche A1.1: Télécharger CEF binaries**
```bash
# Télécharger prébuilt binaries (CEF 122.x stable)
wget https://cef-builds.spotifycdn.com/cef_binary_122.1.11%2Bg5c8b4c2%2Bchromium-122.0.6261.111_linux64.tar.bz2
tar -xjf cef_binary_*.tar.bz2
mv cef_binary_* cef/
```

**Tâche A1.2: Créer CMakeLists.txt**
```cmake
cmake_minimum_required(VERSION 3.16)
project(twake-cef)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# CEF directory
set(CEF_ROOT "${CMAKE_SOURCE_DIR}/cef")

# Find CEF
find_package(CEF REQUIRED)

# Main executable
add_executable(twake_app
    src/main.cpp
    src/app/browser_app.cpp
    src/app/render_app.cpp
    src/browser/window_manager.cpp
    src/browser/js_bridge.cpp
    src/ipc/ipc_client.cpp
)

target_link_libraries(twake_app
    ${CEF_LIBRARIES}
    pthread
)

include_directories(${CEF_INCLUDE_PATH})
```

**Tâche A1.3: Build CEF**
```bash
mkdir build && cd build
cmake ..
make -j$(nproc)
```

**Critère de succès:** Executable `twake_app` créé

---

### Matin (10:00 - 12:00) — Window Management

**Tâche A1.4: CEF initialization**
```cpp
// src/main.cpp
#include "include/cef_app.h"
#include "include/cef_client.h"

int main(int argc, char* argv[]) {
    CefMainArgs args(argc, argv);
    
    CefSettings settings;
    settings.multi_threaded_message_loop = true;
    settings.external_message_pump = false;
    settings.windowless_rendering_enabled = false;
    
    // Disable sandbox for MVP (simplifies build)
    settings.no_sandbox = true;
    
    CefRefPtr<TwakeApp> app = new TwakeApp();
    CefExecuteProcess(args, app.get(), nullptr);
    
    CefInitialize(args, settings, app.get(), nullptr);
    CefRunMessageLoop();
    CefShutdown();
    
    return 0;
}
```

**Tâche A1.5: Create browser**
```cpp
// src/browser/window_manager.cpp
#include "include/cef_app.h"
#include "include/cef_browser.h"
#include "include/cef_frame.h"

class WindowManager {
public:
    void createWindow(const std::string& url) {
        CefWindowInfo windowInfo;
        windowInfo.SetAsWindowless(nullptr);
        
        CefBrowserSettings settings;
        
        CefBrowserHost::CreateBrowser(
            windowInfo,
            this,  // Client
            url,
            settings,
            nullptr  // Request context
        );
    }
    
    void createBrowser(CefRefPtr<CefApp> app, const std::string& url) {
        CefWindowInfo windowInfo;
        windowInfo.SetAsWindow(nullptr, 800, 600);
        
        CefBrowserSettings settings;
        
        CefBrowserHost::CreateBrowserSync(
            windowInfo,
            new TwakeClientHandler(),
            url,
            settings,
            nullptr,
            CefPoint()
        );
    }
};
```

**Tâche A1.6: Client handler**
```cpp
// src/browser/window_manager.h
class TwakeClientHandler : public CefClient {
public:
    TwakeClientHandler() : render_delegate_(new TwakeRenderDelegate()) {}
    
    CefRefPtr<CefRenderProcessHandler> GetRenderProcessHandler() override {
        return render_delegate_;
    }
    
private:
    CefRefPtr<TwakeRenderDelegate> render_delegate_;
    
    IMPLEMENT_REFCOUNTING(TwakeClientHandler);
};
```

**Critère de succès:** Fenêtre CEF s'ouvre avec URL chargée

---

### Après-midi (14:00 - 16:00) — IPC Client

**Tâche A1.7: IPC client setup**
```cpp
// src/ipc/ipc_client.h
#include <string>
#include <nlohmann/json.hpp>

class IpcClient {
public:
    bool connect(const std::string& socket_path);
    void disconnect();
    
    nlohmann::json callMethod(const std::string& method, 
                               const nlohmann::json& params);
    
private:
    int socket_fd_;
    bool connected_;
};
```

**Tâche A1.8: IPC client implementation**
```cpp
// src/ipc/ipc_client.cpp
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

bool IpcClient::connect(const std::string& socket_path) {
    socket_fd_ = socket(AF_UNIX, SOCK_STREAM, 0);
    if (socket_fd_ < 0) return false;
    
    struct sockaddr_un addr;
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, socket_path.c_str(), sizeof(addr.sun_path) - 1);
    
    int result = connect(socket_fd_, 
                        (struct sockaddr*)&addr, 
                        sizeof(addr));
    return result == 0;
}

nlohmann::json IpcClient::callMethod(const std::string& method,
                                      const nlohmann::json& params) {
    nlohmann::json request = {
        {"jsonrpc", "2.0"},
        {"method", method},
        {"params", params},
        {"id", ++request_id_}
    };
    
    std::string request_str = request.dump();
    send(socket_fd_, request_str.c_str(), request_str.size(), 0);
    
    char buffer[4096];
    int bytes_received = recv(socket_fd_, buffer, sizeof(buffer), 0);
    
    return nlohmann::json::parse(std::string(buffer, bytes_received));
}
```

**Tâche A1.9: Wrapper methods**
```cpp
// src/ipc/ipc_client.cpp
class TwakeIpcClient : public IpcClient {
public:
    FileStatus getFileStatus(const std::string& path) {
        auto result = callMethod("file.status", {{"path", path}});
        return FileStatus::fromJson(result);
    }
    
    bool hydrateFile(const std::string& path) {
        auto result = callMethod("file.hydrate", {{"path", path}});
        return result["success"];
    }
    
    std::vector<FileNode> listFiles(const std::string& path, 
                                     bool recursive = false) {
        auto result = callMethod("file.list", {{"path", path}, {"recursive", recursive}});
        return FileNode::fromJsonArray(result);
    }
};
```

**Critère de succès:** IPC client peut se connecter et appeler méthode dummy

---

### Après-midi (16:00 - 18:00) — Bridge JS

**Tâche A1.10: Render process delegate**
```cpp
// src/app/render_app.cpp
#include "include/cef_app.h"
#include "include/cef_v8.h"

class TwakeRenderDelegate : public CefRenderProcessHandler {
public:
    void OnContextCreated(CefRefPtr<CefBrowser> browser,
                         CefRefPtr<CefFrame> frame,
                         CefRefPtr<CefV8Context> context) override {
        
        std::string url = frame->GetURL();
        if (!isTwakeDomain(url)) {
            return;
        }
        
        CefRefPtr<CefV8Value> twake = CefV8Value::CreateObject();
        
        // Register methods
        twake->SetValue("getFileStatus",
            CefV8Handler::Create([this](const CefV8ValueList& args) {
                std::string path = args[0]->GetStringValue();
                // Call IPC client
                return callIpcMethod("file.status", {{"path", path}});
            }),
            V8_PROPERTY_ATTRIBUTE_READONLY);
        
        twake->SetValue("hydrateFile",
            CefV8Handler::Create([this](const CefV8ValueList& args) {
                std::string path = args[0]->GetStringValue();
                return callIpcMethod("file.hydrate", {{"path", path}});
            }),
            V8_PROPERTY_ATTRIBUTE_READONLY);
        
        context->GetGlobal()->SetValue("__twake", twake, 
                                       V8_PROPERTY_ATTRIBUTE_NONE);
    }
    
private:
    bool isTwakeDomain(const std::string& url) {
        return url.find("twake.app") != std::string::npos;
    }
    
    CefRefPtr<CefV8Value> callIpcMethod(const std::string& method,
                                          const nlohmann::json& params) {
        // Call IPC client and return result
        auto result = ipc_client_->callMethod(method, params);
        return CefV8Value::CreateString(result.dump());
    }
};
```

**Tâche A1.11: JS helper library**
```javascript
// src/assets/twake-client.js
class TwakeClient {
  constructor() {
    this.eventHandlers = new Map();
  }

  getFileStatus(path) {
    const result = window.__twake.getFileStatus(path);
    return JSON.parse(result);
  }

  async hydrateFile(path) {
    const result = window.__twake.hydrateFile(path);
    return JSON.parse(result);
  }

  on(event, callback) {
    if (!this.eventHandlers.has(event)) {
      this.eventHandlers.set(event, []);
    }
    this.eventHandlers.get(event).push(callback);
  }

  emit(event, data) {
    window.__twake.emit(event, JSON.stringify(data));
  }
}

export const twake = new TwakeClient();
```

**Critère de succès:** `window.__twake` injecté dans Twake WebViews

---

## Jour 2 — Integration et UI

### Matin (08:00 - 10:00) — Login UI

**Tâche A2.1: Tray icon (optionnel)**
```cpp
// src/browser/tray_icon.cpp
#include "include/cef_app.h"

void createTrayIcon() {
    // Linux: Use libappindicator or system tray
    // MVP: Skip, just console log
}
```

**Tâche A2.2: Login button**
```html
<!-- src/assets/login.html -->
<!DOCTYPE html>
<html>
<head>
    <title>Twake Desktop</title>
</head>
<body>
    <button id="loginBtn">Login with OIDC</button>
    <script>
        document.getElementById('loginBtn').addEventListener('click', async () => {
            // Open browser for OIDC
            window.open('https://sso.company.com/oauth2/auth?...', '_blank');
            
            // Listen for callback
            window.addEventListener('message', (event) => {
                if (event.data.type === 'auth_callback') {
                    // Store token
                    localStorage.setItem('access_token', event.data.token);
                    window.location.href = 'https://app1.twake.app';
                }
            });
        });
    </script>
</body>
</html>
```

**Tâche A2.3: Token storage**
```cpp
// src/browser/token_storage.cpp
#include <fstream>
#include <filesystem>

class TokenStorage {
public:
    bool saveToken(const std::string& token) {
        std::ofstream file(getTokenPath());
        file << token;
        return file.good();
    }
    
    std::string getToken() {
        std::ifstream file(getTokenPath());
        return std::string((std::istreambuf_iterator<char>(file)),
                          std::istreambuf_iterator<char>());
    }
    
private:
    std::string getTokenPath() {
        return std::filesystem::home_directory() / ".twake" / "token";
    }
};
```

**Critère de succès:** Login button ouvre navigateur OIDC

---

### Matin (10:00 - 12:00) — Event Dispatch

**Tâche A2.4: Event listener from Rust**
```cpp
// src/browser/js_bridge.cpp
void JsBridge::onEvent(const std::string& event, const std::string& data) {
    // Dispatch to all browsers
    for (auto& browser : browsers_) {
        CefRefPtr<CefProcessMessage> message = 
            CefProcessMessage::Create("TWAKE_EVENT");
        
        CefArgList& args = message->GetArgumentList();
        args.SetString(0, event);
        args.SetString(1, data);
        
        browser->GetMainFrame()->SendProcessMessage(
            CefProcessId::PID_RENDERER, message);
    }
}
```

**Tâche A2.5: JS event handler**
```cpp
// src/app/render_app.cpp
void TwakeRenderDelegate::OnProcessMessageReceived(
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

**Critère de succès:** Events de Rust arrivent en JS

---

### Après-midi (14:00 - 18:00) — Demo Prep

**Tâche A2.6: Demo script**
```bash
#!/bin/bash
# demo.sh

echo "1. Launching Twake Desktop..."
./build/twake_app &
sleep 3

echo "2. Opening Twake Drive..."
# Click on Drive icon

echo "3. Showing ghost files..."
# Show /mnt/twake/documents

echo "4. Hydrating file..."
# Right-click on test.txt → Hydrate

echo "5. File downloaded!"
# Show file content
```

**Tâche A2.7: Error handling**
```cpp
// src/ipc/ipc_client.cpp
nlohmann::json IpcClient::callMethod(const std::string& method,
                                      const nlohmann::json& params) {
    if (!connected_) {
        LOG(ERROR) << "[IPC] Not connected, retrying...";
        if (!connect("/tmp/twake-ipc.sock")) {
            throw std::runtime_error("IPC connection failed");
        }
    }
    
    try {
        return sendRequest(method, params);
    } catch (...) {
        LOG(ERROR) << "[IPC] Request failed, reconnecting...";
        connected_ = false;
        throw;
    }
}
```

**Tâche A2.8: Logging**
```cpp
// src/main.cpp
void setupLogging() {
    // Simple console logging for MVP
    #ifdef DEBUG
    CefSettings.log_severity = LOGSEVERITY_INFO;
    #else
    CefSettings.log_severity = LOGSEVERITY_WARNING;
    #endif
}
```

**Critère de succès:** Demo de 5 minutes sans crash

---

## Build Commands

```bash
# Build
mkdir build && cd build
cmake ..
make -j$(nproc)

# Run
./twake_app

# Debug
gdb ./twake_app
```

## Dependencies

```cmake
# Required
- CEF 122.x
- nlohmann/json (header-only)
- pthread

# Optional
- libappindicator (tray icon)
```

## Known Issues

- **Sandbox:** Désactivé pour MVP (`no_sandbox = true`)
- **Tray icon:** Skip pour Linux MVP
- **Auto-update:** Pas implémenté
