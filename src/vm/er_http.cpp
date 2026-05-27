#include "App.h"
#include <string>
#include <string_view>
#include <iostream>

extern "C" {
    void er_http_on_request(void* res, const char* method, size_t method_len, const char* path, size_t path_len);
    void er_ws_on_open(void* ws, const char* path, size_t path_len);
    void er_ws_on_message(void* ws, const char* path, size_t path_len, const char* message, size_t message_len);
    void er_ws_on_close(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);
}

struct PerSocketData {
    // Fill with user data if needed
};

// Global app pointer (without SSL support)
static uWS::App* g_app = nullptr;

extern "C" void er_http_init() {
    if (g_app) {
        delete g_app;
    }
    g_app = new uWS::App();
}

extern "C" void er_ws_register_route(const char* path) {
    if (!g_app) return;
    
    std::string path_str(path);
    
    g_app->ws<PerSocketData>(path_str, {
        .compression = uWS::CompressOptions(uWS::SHARED_COMPRESSOR),
        .maxPayloadLength = 16 * 1024 * 1024,
        .idleTimeout = 120,
        .maxBackpressure = 16 * 1024 * 1024,
        .closeOnBackpressureLimit = false,
        .resetIdleTimeoutOnSend = false,
        .sendPingsAutomatically = true,
        .open = [path_str](auto* ws) {
            er_ws_on_open(ws, path_str.data(), path_str.length());
        },
        .message = [path_str](auto* ws, std::string_view message, uWS::OpCode opCode) {
            er_ws_on_message(ws, path_str.data(), path_str.length(), message.data(), message.length());
        },
        .close = [path_str](auto* ws, int code, std::string_view message) {
            er_ws_on_close(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
        }
    });
}

extern "C" void er_ws_send(void* ws, const char* message, size_t message_len) {
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    web_socket->send(std::string_view(message, message_len), uWS::OpCode::TEXT, false);
}

extern "C" void er_ws_close(void* ws) {
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    web_socket->close();
}

extern "C" void er_http_register_route(const char* method, const char* path) {
    if (!g_app) return;
    
    std::string method_str(method);
    std::string path_str(path);
    
    if (method_str == "GET") {
        g_app->get(path_str, [](auto* res, auto* req) {
            std::string_view method = "GET";
            std::string_view path = req->getUrl();
            
            res->onAborted([]() {});
            er_http_on_request(res, method.data(), method.length(), path.data(), path.length());
        });
    } else if (method_str == "POST") {
        g_app->post(path_str, [](auto* res, auto* req) {
            std::string_view method = "POST";
            std::string_view path = req->getUrl();
            
            res->onAborted([]() {});
            er_http_on_request(res, method.data(), method.length(), path.data(), path.length());
        });
    }
}

extern "C" void er_http_listen_and_run(int port) {
    if (!g_app) return;
    
    g_app->get("/*", [](auto* res, auto* req) {
        std::string_view method = "GET";
        std::string_view path = req->getUrl();
        res->onAborted([]() {});
        er_http_on_request(res, method.data(), method.length(), path.data(), path.length());
    });
    
    g_app->post("/*", [](auto* res, auto* req) {
        std::string_view method = "POST";
        std::string_view path = req->getUrl();
        res->onAborted([]() {});
        er_http_on_request(res, method.data(), method.length(), path.data(), path.length());
    });
    
    g_app->listen(port, [port](auto* listen_socket) {
        if (listen_socket) {
            std::cout << "[uWebSockets] Server listening on port " << port << std::endl;
        } else {
            std::cerr << "[uWebSockets] Failed to listen on port " << port << std::endl;
        }
    }).run();
}

extern "C" void er_http_response_end_json(void* res, const char* json_str, size_t json_len) {
    auto* http_res = static_cast<uWS::HttpResponse<false>*>(res);
    http_res->writeHeader("Content-Type", "application/json");
    http_res->end(std::string_view(json_str, json_len));
}
