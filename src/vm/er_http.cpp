#include "App.h"
#include <string>
#include <string_view>
#include <iostream>

extern "C" {
    void er_http_on_request(void* res, const char* method, size_t method_len, const char* path, size_t path_len);
}

// Global app pointer (without SSL support)
static uWS::App* g_app = nullptr;

extern "C" void er_http_init() {
    if (g_app) {
        delete g_app;
    }
    g_app = new uWS::App();
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
