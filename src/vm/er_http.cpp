#include "App.h"
#include <string>
#include <string_view>
#include <iostream>
#include <cstring>
#include <memory>

extern "C" {
    void er_http_on_request(void* res, const char* method, size_t method_len, const char* path, size_t path_len, const char* headers, size_t headers_len, const char* body, size_t body_len);
    void er_ws_on_open(void* ws, const char* path, size_t path_len);
    void er_ws_on_message(void* ws, const char* path, size_t path_len, const char* message, size_t message_len);
    void er_ws_on_close(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);
    void er_http_on_listening();
}

typedef void (*HttpRequestCallback)(void* res, const char* method, size_t method_len, const char* path, size_t path_len, const char* headers, size_t headers_len, const char* body, size_t body_len);
typedef void (*WsOpenCallback)(void* ws, const char* path, size_t path_len);
typedef void (*WsMessageCallback)(void* ws, const char* path, size_t path_len, const char* message, size_t message_len);
typedef void (*WsCloseCallback)(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);

static HttpRequestCallback g_http_req_cb = nullptr;
static WsOpenCallback g_ws_open_cb = nullptr;
static WsMessageCallback g_ws_message_cb = nullptr;
static WsCloseCallback g_ws_close_cb = nullptr;

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
    g_http_req_cb = nullptr;
    g_ws_open_cb = nullptr;
    g_ws_message_cb = nullptr;
    g_ws_close_cb = nullptr;
}

extern "C" void er_http_init_with_callbacks(
    HttpRequestCallback http_req_cb,
    WsOpenCallback ws_open_cb,
    WsMessageCallback ws_message_cb,
    WsCloseCallback ws_close_cb
) {
    if (g_app) {
        delete g_app;
    }
    g_app = new uWS::App();
    g_http_req_cb = http_req_cb;
    g_ws_open_cb = ws_open_cb;
    g_ws_message_cb = ws_message_cb;
    g_ws_close_cb = ws_close_cb;
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
            if (g_ws_open_cb) {
                g_ws_open_cb(ws, path_str.data(), path_str.length());
            } else {
                er_ws_on_open(ws, path_str.data(), path_str.length());
            }
        },
        .message = [path_str](auto* ws, std::string_view message, uWS::OpCode opCode) {
            if (g_ws_message_cb) {
                g_ws_message_cb(ws, path_str.data(), path_str.length(), message.data(), message.length());
            } else {
                er_ws_on_message(ws, path_str.data(), path_str.length(), message.data(), message.length());
            }
        },
        .close = [path_str](auto* ws, int code, std::string_view message) {
            if (g_ws_close_cb) {
                g_ws_close_cb(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
            } else {
                er_ws_on_close(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
            }
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
            
            std::string headers_str;
            for (auto h : *req) {
                headers_str.append(h.first);
                headers_str.append(": ");
                headers_str.append(h.second);
                headers_str.append("\r\n");
            }
            
            res->onAborted([]() {});
            if (g_http_req_cb) {
                g_http_req_cb(res, method.data(), method.length(), path.data(), path.length(),
                              headers_str.data(), headers_str.length(), nullptr, 0);
            } else {
                er_http_on_request(res, method.data(), method.length(), path.data(), path.length(),
                                   headers_str.data(), headers_str.length(), nullptr, 0);
            }
        });
    } else if (method_str == "POST") {
        g_app->post(path_str, [](auto* res, auto* req) {
            std::string_view method = "POST";
            std::string_view path = req->getUrl();
            
            std::string headers_str;
            for (auto h : *req) {
                headers_str.append(h.first);
                headers_str.append(": ");
                headers_str.append(h.second);
                headers_str.append("\r\n");
            }
            
            struct PostCtx {
                std::string method;
                std::string path;
                std::string headers;
                std::string body;
                bool aborted = false;
            };
            auto ctx = std::make_shared<PostCtx>();
            ctx->method = std::string(method);
            ctx->path = std::string(path);
            ctx->headers = std::move(headers_str);
            
            res->onAborted([ctx]() {
                ctx->aborted = true;
            });
            
            res->onData([ctx, res](std::string_view chunk, bool isLast) {
                if (ctx->aborted) return;
                ctx->body.append(chunk.data(), chunk.length());
                if (isLast) {
                    if (g_http_req_cb) {
                        g_http_req_cb(res, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                      ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                    } else {
                        er_http_on_request(res, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                           ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                    }
                }
            });
        });
    }
}

extern "C" void er_http_listen_and_run(int port) {
    if (!g_app) return;
    
    g_app->get("/*", [](auto* res, auto* req) {
        std::string_view method = "GET";
        std::string_view path = req->getUrl();
        
        std::string headers_str;
        for (auto h : *req) {
            headers_str.append(h.first);
            headers_str.append(": ");
            headers_str.append(h.second);
            headers_str.append("\r\n");
        }
        
        res->onAborted([]() {});
        if (g_http_req_cb) {
            g_http_req_cb(res, method.data(), method.length(), path.data(), path.length(),
                          headers_str.data(), headers_str.length(), nullptr, 0);
        } else {
            er_http_on_request(res, method.data(), method.length(), path.data(), path.length(),
                               headers_str.data(), headers_str.length(), nullptr, 0);
        }
    });
    
    g_app->post("/*", [](auto* res, auto* req) {
        std::string_view method = "POST";
        std::string_view path = req->getUrl();
        
        std::string headers_str;
        for (auto h : *req) {
            headers_str.append(h.first);
            headers_str.append(": ");
            headers_str.append(h.second);
            headers_str.append("\r\n");
        }
        
        struct PostCtx {
            std::string method;
            std::string path;
            std::string headers;
            std::string body;
            bool aborted = false;
        };
        auto ctx = std::make_shared<PostCtx>();
        ctx->method = std::string(method);
        ctx->path = std::string(path);
        ctx->headers = std::move(headers_str);
        
        res->onAborted([ctx]() {
            ctx->aborted = true;
        });
        
        res->onData([ctx, res](std::string_view chunk, bool isLast) {
            if (ctx->aborted) return;
            ctx->body.append(chunk.data(), chunk.length());
            if (isLast) {
                if (g_http_req_cb) {
                    g_http_req_cb(res, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                  ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                } else {
                    er_http_on_request(res, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                       ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                }
            }
        });
    });
    
    g_app->listen(port, [port](auto* listen_socket) {
        if (listen_socket) {
            er_http_on_listening();
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

extern "C" void er_http_response_end_html(void* res, const char* html_str, size_t html_len) {
    auto* http_res = static_cast<uWS::HttpResponse<false>*>(res);
    http_res->writeHeader("Content-Type", "text/html; charset=utf-8");
    http_res->end(std::string_view(html_str, html_len));
}

extern "C" void er_http_response_write_status(void* res, const char* status_str, size_t status_len) {
    auto* http_res = static_cast<uWS::HttpResponse<false>*>(res);
    http_res->writeStatus(std::string_view(status_str, status_len));
}

extern "C" void er_http_response_write_header(void* res, const char* key_str, size_t key_len, const char* val_str, size_t val_len) {
    auto* http_res = static_cast<uWS::HttpResponse<false>*>(res);
    http_res->writeHeader(std::string_view(key_str, key_len), std::string_view(val_str, val_len));
}

extern "C" void er_http_response_end(void* res, const char* data_str, size_t data_len) {
    auto* http_res = static_cast<uWS::HttpResponse<false>*>(res);
    http_res->end(std::string_view(data_str, data_len));
}

extern "C" void er_http_create_timer(int ms, void (*cb)(void*)) {
    auto* loop = uWS::Loop::get();
    struct us_timer_t *timer = us_create_timer((struct us_loop_t *) loop, 0, sizeof(void (*)(void*)));
    std::memcpy(us_timer_ext(timer), &cb, sizeof(void (*)(void*)));
    us_timer_set(timer, [](struct us_timer_t *t) {
        void (*cb)(void*);
        std::memcpy(&cb, us_timer_ext(t), sizeof(void (*)(void*)));
        cb(t);
    }, ms, ms);
}

