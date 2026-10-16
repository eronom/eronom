#include "App.h"
#include <string>
#include <string_view>
#include <iostream>
#include <cstring>
#include <memory>
#include <atomic>
#include <cstdint>
#include <vector>

#ifdef _MSC_VER
#define strncasecmp _strnicmp
#define strcasecmp _stricmp
#endif

#if defined(__GNUC__) || defined(__clang__)
#define ER_WEAK __attribute__((weak))
#else
#define ER_WEAK
#endif

extern "C" {
    // Legacy callbacks
    ER_WEAK void er_http_on_request(void*, const char*, size_t, const char*, size_t, const char*, size_t, const char*, size_t) {}
    ER_WEAK void er_ws_on_open(void*, const char*, size_t) {}
    ER_WEAK void er_ws_on_message(void*, const char*, size_t, const char*, size_t, int) {}
    ER_WEAK void er_ws_on_close(void*, const char*, size_t, int, const char*, size_t) {}
    ER_WEAK void er_http_on_listening() {}

    // Direct dispatch callbacks
    ER_WEAK void er_http_on_route(
        void*,
        uint32_t,
        void*,
        void*,
        const char*,
        size_t,
        const char*,
        size_t
    ) {}

    ER_WEAK void er_http_on_fallback(
        void*,
        void*,
        void*,
        const char*,
        size_t,
        const char*,
        size_t,
        const char*,
        size_t
    ) {}

    ER_WEAK void er_http_on_listening_instance(void*, int) {}
    ER_WEAK void er_ws_on_open_instance(void*, uint32_t, void*, const char*, size_t) {}
    ER_WEAK void er_ws_on_message_instance(void*, uint32_t, void*, const char*, size_t, const char*, size_t, int) {}
    ER_WEAK void er_ws_on_close_instance(void*, uint32_t, void*, const char*, size_t, int, const char*, size_t) {}
}

typedef void (*HttpRequestCallback)(void* res, const char* method, size_t method_len, const char* path, size_t path_len, const char* headers, size_t headers_len, const char* body, size_t body_len);
typedef void (*WsOpenCallback)(void* ws, const char* path, size_t path_len);
typedef void (*WsMessageCallback)(void* ws, const char* path, size_t path_len, const char* message, size_t message_len, int is_binary);
typedef void (*WsCloseCallback)(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);

static HttpRequestCallback g_http_req_cb = nullptr;
static WsOpenCallback g_ws_open_cb = nullptr;
static WsMessageCallback g_ws_message_cb = nullptr;
static WsCloseCallback g_ws_close_cb = nullptr;

struct PerSocketData {
    // User socket context
};

struct HttpResponseToken {
    std::atomic<uWS::HttpResponse<false>*> res;
    std::atomic<bool> aborted{false};
    std::atomic<bool> responded{false};
    std::atomic<uint32_t> ref_count{2}; // 1 for uWS onAborted wrapper, 1 for Rust context

    HttpResponseToken(uWS::HttpResponse<false>* r) : res(r) {}

    void add_ref() {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    void release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            delete this;
        }
    }
};

struct AbortHandler {
    HttpResponseToken* token;
    explicit AbortHandler(HttpResponseToken* t) : token(t) {}
    AbortHandler(const AbortHandler& o) : token(o.token) {
        if (token) token->add_ref();
    }
    AbortHandler(AbortHandler&& o) noexcept : token(o.token) {
        o.token = nullptr;
    }
    AbortHandler& operator=(const AbortHandler& o) {
        if (this != &o) {
            if (token) token->release();
            token = o.token;
            if (token) token->add_ref();
        }
        return *this;
    }
    AbortHandler& operator=(AbortHandler&& o) noexcept {
        if (this != &o) {
            if (token) token->release();
            token = o.token;
            o.token = nullptr;
        }
        return *this;
    }
    ~AbortHandler() {
        if (token) {
            token->release();
            token = nullptr;
        }
    }
    void operator()() {
        if (token) {
            token->aborted.store(true, std::memory_order_release);
            token->res.store(nullptr, std::memory_order_release);
        }
    }
};

// Unified zero-copy request view
struct ErReqView {
    bool is_live{true};
    uWS::HttpRequest* live_req{nullptr};
    std::vector<std::pair<std::string, std::string>> saved_headers;
    std::string saved_url;
    std::string saved_query;
    std::string saved_method;
};

struct RouteBodyCtx {
    HttpResponseToken* token;
    std::string url;
    std::string body;
    ErReqView view;
    explicit RouteBodyCtx(HttpResponseToken* t) : token(t) { if (token) token->add_ref(); }
    ~RouteBodyCtx() { if (token) token->release(); }
};

struct FallbackDevCtx {
    HttpResponseToken* token;
    std::string method;
    std::string url;
    std::string headers;
    std::string body;
    explicit FallbackDevCtx(HttpResponseToken* t) : token(t) { if (token) token->add_ref(); }
    ~FallbackDevCtx() { if (token) token->release(); }
};

struct FallbackCtx {
    HttpResponseToken* token;
    std::string method;
    std::string url;
    std::string body;
    ErReqView view;
    explicit FallbackCtx(HttpResponseToken* t) : token(t) { if (token) token->add_ref(); }
    ~FallbackCtx() { if (token) token->release(); }
};

// ─── Zero-Copy Request Inspection APIs ───────────────────────────────────────

extern "C" {

bool er_http_req_get_header(void* req_ptr, const char* key, size_t key_len, const char** out_val, size_t* out_len) {
    if (!req_ptr || !key || !out_val || !out_len) return false;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        std::string_view val = view->live_req->getHeader(std::string_view(key, key_len));
        if (val.data() != nullptr) {
            *out_val = val.data();
            *out_len = val.length();
            return true;
        }
        return false;
    }
    for (const auto& h : view->saved_headers) {
        if (h.first.length() == key_len && strncasecmp(h.first.data(), key, key_len) == 0) {
            *out_val = h.second.data();
            *out_len = h.second.length();
            return true;
        }
    }
    return false;
}

typedef void (*HeaderIterCallback)(void* user_data, const char* key, size_t key_len, const char* val, size_t val_len);

void er_http_req_for_each_header(void* req_ptr, HeaderIterCallback cb, void* user_data) {
    if (!req_ptr || !cb) return;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        for (auto h : *(view->live_req)) {
            cb(user_data, h.first.data(), h.first.length(), h.second.data(), h.second.length());
        }
    } else {
        for (const auto& h : view->saved_headers) {
            cb(user_data, h.first.data(), h.first.length(), h.second.data(), h.second.length());
        }
    }
}

void er_http_req_get_url(void* req_ptr, const char** out_url, size_t* out_len) {
    if (!req_ptr || !out_url || !out_len) return;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        std::string_view u = view->live_req->getUrl();
        *out_url = u.data();
        *out_len = u.length();
    } else {
        *out_url = view->saved_url.data();
        *out_len = view->saved_url.length();
    }
}

void er_http_req_get_query(void* req_ptr, const char** out_q, size_t* out_len) {
    if (!req_ptr || !out_q || !out_len) return;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        std::string_view q = view->live_req->getQuery();
        *out_q = q.data();
        *out_len = q.length();
    } else {
        *out_q = view->saved_query.data();
        *out_len = view->saved_query.length();
    }
}

void er_http_req_get_method(void* req_ptr, const char** out_m, size_t* out_len) {
    if (!req_ptr || !out_m || !out_len) return;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        std::string_view m = view->live_req->getCaseSensitiveMethod();
        *out_m = m.data();
        *out_len = m.length();
    } else {
        *out_m = view->saved_method.data();
        *out_len = view->saved_method.length();
    }
}

bool er_http_req_get_parameter(void* req_ptr, unsigned short index, const char** out_p, size_t* out_len) {
    if (!req_ptr || !out_p || !out_len) return false;
    auto* view = static_cast<ErReqView*>(req_ptr);
    if (view->is_live && view->live_req) {
        std::string_view p = view->live_req->getParameter(index);
        if (p.data() != nullptr) {
            *out_p = p.data();
            *out_len = p.length();
            return true;
        }
    }
    return false;
}

} // extern "C"

// ─── ErServer Structure & Instance Lifecycle ─────────────────────────────────

struct ErServer {
    bool ssl{false};
    uWS::App* app{nullptr};
    void* rust_server{nullptr};
    us_listen_socket_t* listen_socket{nullptr};

    ErServer(bool is_ssl, void* r_server) : ssl(is_ssl), rust_server(r_server) {
        app = new uWS::App();
    }

    ~ErServer() {
        if (listen_socket) {
            us_listen_socket_close(ssl ? 1 : 0, listen_socket);
            listen_socket = nullptr;
        }
        if (app) {
            delete app;
            app = nullptr;
        }
    }
};

static ErServer* g_default_server = nullptr;
static uWS::App* g_app = nullptr;

extern "C" {

void* er_server_create(int ssl, void* rust_server) {
    return new ErServer(ssl != 0, rust_server);
}

void er_server_destroy(void* server_ptr) {
    if (server_ptr) {
        delete static_cast<ErServer*>(server_ptr);
    }
}

void er_server_register_route(void* server_ptr, const char* method, const char* path, uint32_t route_id) {
    if (!server_ptr || !method || !path) return;
    auto* server = static_cast<ErServer*>(server_ptr);
    auto* app = server->app;
    if (!app) return;

    std::string method_str(method);
    for (char &c : method_str) c = (char)toupper((unsigned char)c);
    std::string path_str(path);
    void* rust_server = server->rust_server;

    if (method_str == "GET") {
        app->get(path_str, [rust_server, route_id](auto* res, auto* req) {
            auto* token = new HttpResponseToken(res);
            res->onAborted(AbortHandler(token));
            std::string_view full_url = req->getFullUrl();
            ErReqView view;
            view.is_live = true;
            view.live_req = req;
            er_http_on_route(rust_server, route_id, token, &view, full_url.data(), full_url.length(), nullptr, 0);
        });
    } else if (method_str == "HEAD") {
        app->head(path_str, [rust_server, route_id](auto* res, auto* req) {
            auto* token = new HttpResponseToken(res);
            res->onAborted(AbortHandler(token));
            std::string_view full_url = req->getFullUrl();
            ErReqView view;
            view.is_live = true;
            view.live_req = req;
            er_http_on_route(rust_server, route_id, token, &view, full_url.data(), full_url.length(), nullptr, 0);
        });
    } else {
        auto register_body = [app, rust_server, route_id, &path_str](auto attach_fn) {
            attach_fn(path_str, [rust_server, route_id](auto* res, auto* req) {
                auto* token = new HttpResponseToken(res);
                res->onAborted(AbortHandler(token));
                std::string full_url(req->getFullUrl());

                auto ctx = std::make_shared<RouteBodyCtx>(token);
                ctx->url = std::move(full_url);
                ctx->view.is_live = false;
                ctx->view.saved_url = ctx->url;
                std::string_view q = req->getQuery();
                ctx->view.saved_query = std::string(q);
                std::string_view m = req->getCaseSensitiveMethod();
                ctx->view.saved_method = std::string(m);
                for (auto h : *req) {
                    ctx->view.saved_headers.push_back({std::string(h.first), std::string(h.second)});
                }

                res->onData([ctx, token, rust_server, route_id](std::string_view chunk, bool isLast) {
                    if (token->aborted.load(std::memory_order_acquire)) return;
                    ctx->body.append(chunk.data(), chunk.length());
                    if (isLast) {
                        er_http_on_route(rust_server, route_id, token, &ctx->view,
                                         ctx->url.data(), ctx->url.length(),
                                         ctx->body.data(), ctx->body.length());
                    }
                });
            });
        };

        if (method_str == "POST") {
            register_body([app](const std::string& p, auto h) { app->post(p, std::move(h)); });
        } else if (method_str == "PUT") {
            register_body([app](const std::string& p, auto h) { app->put(p, std::move(h)); });
        } else if (method_str == "PATCH") {
            register_body([app](const std::string& p, auto h) { app->patch(p, std::move(h)); });
        } else if (method_str == "DELETE" || method_str == "DEL") {
            register_body([app](const std::string& p, auto h) { app->del(p, std::move(h)); });
        } else if (method_str == "OPTIONS") {
            register_body([app](const std::string& p, auto h) { app->options(p, std::move(h)); });
        } else if (method_str == "ALL" || method_str == "ANY" || method_str == "*") {
            register_body([app](const std::string& p, auto h) { app->any(p, std::move(h)); });
        }
    }
}

void er_server_register_ws_route(void* server_ptr, const char* path, uint32_t route_id) {
    if (!server_ptr || !path) return;
    auto* server = static_cast<ErServer*>(server_ptr);
    auto* app = server->app;
    if (!app) return;

    std::string path_str(path);
    void* rust_server = server->rust_server;

    uWS::App::WebSocketBehavior<PerSocketData> ws_behavior;
    ws_behavior.compression = uWS::CompressOptions(uWS::SHARED_COMPRESSOR);
    ws_behavior.maxPayloadLength = 16 * 1024 * 1024;
    ws_behavior.idleTimeout = 120;
    ws_behavior.maxBackpressure = 16 * 1024 * 1024;
    ws_behavior.closeOnBackpressureLimit = false;
    ws_behavior.resetIdleTimeoutOnSend = false;
    ws_behavior.sendPingsAutomatically = true;

    ws_behavior.open = [rust_server, route_id, path_str](auto* ws) {
        if (g_ws_open_cb) {
            g_ws_open_cb(ws, path_str.data(), path_str.length());
        } else {
            er_ws_on_open_instance(rust_server, route_id, ws, path_str.data(), path_str.length());
        }
    };
    ws_behavior.message = [rust_server, route_id, path_str](auto* ws, std::string_view message, uWS::OpCode opCode) {
        int is_binary = (opCode == uWS::OpCode::BINARY) ? 1 : 0;
        if (g_ws_message_cb) {
            g_ws_message_cb(ws, path_str.data(), path_str.length(), message.data(), message.length(), is_binary);
        } else {
            er_ws_on_message_instance(rust_server, route_id, ws, path_str.data(), path_str.length(), message.data(), message.length(), is_binary);
        }
    };
    ws_behavior.close = [rust_server, route_id, path_str](auto* ws, int code, std::string_view message) {
        if (g_ws_close_cb) {
            g_ws_close_cb(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
        } else {
            er_ws_on_close_instance(rust_server, route_id, ws, path_str.data(), path_str.length(), code, message.data(), message.length());
        }
    };

    app->ws<PerSocketData>(path_str, std::move(ws_behavior));
}

static void register_fallback_internal(ErServer* server) {
    if (!server || !server->app) return;
    auto* app = server->app;
    void* rust_server = server->rust_server;

    app->any("/*", [rust_server](auto* res, auto* req) {
        auto* token = new HttpResponseToken(res);
        res->onAborted(AbortHandler(token));

        std::string method_str(req->getCaseSensitiveMethod());
        for (char &c : method_str) c = (char)toupper((unsigned char)c);
        std::string full_url(req->getFullUrl());

        if (g_http_req_cb) {
            std::string headers_str;
            for (auto h : *req) {
                headers_str.append(h.first).append(": ").append(h.second).append("\r\n");
            }
            if (method_str == "GET" || method_str == "HEAD") {
                g_http_req_cb(token, method_str.data(), method_str.length(),
                              full_url.data(), full_url.length(),
                              headers_str.data(), headers_str.length(),
                              nullptr, 0);
            } else {
                auto ctx = std::make_shared<FallbackDevCtx>(token);
                ctx->method = std::move(method_str);
                ctx->url = std::move(full_url);
                ctx->headers = std::move(headers_str);

                res->onData([ctx, token](std::string_view chunk, bool isLast) {
                    if (token->aborted.load(std::memory_order_acquire)) return;
                    ctx->body.append(chunk.data(), chunk.length());
                    if (isLast) {
                        g_http_req_cb(token, ctx->method.data(), ctx->method.length(),
                                      ctx->url.data(), ctx->url.length(),
                                      ctx->headers.data(), ctx->headers.length(),
                                      ctx->body.data(), ctx->body.length());
                    }
                });
            }
            return;
        }

        if (method_str == "GET" || method_str == "HEAD") {
            ErReqView view;
            view.is_live = true;
            view.live_req = req;
            er_http_on_fallback(rust_server, token, &view,
                                method_str.data(), method_str.length(),
                                full_url.data(), full_url.length(),
                                nullptr, 0);
        } else {
            auto ctx = std::make_shared<FallbackCtx>(token);
            ctx->method = std::move(method_str);
            ctx->url = std::move(full_url);
            ctx->view.is_live = false;
            ctx->view.saved_url = ctx->url;
            std::string_view q = req->getQuery();
            ctx->view.saved_query = std::string(q);
            ctx->view.saved_method = ctx->method;
            for (auto h : *req) {
                ctx->view.saved_headers.push_back({std::string(h.first), std::string(h.second)});
            }

            res->onData([ctx, token, rust_server](std::string_view chunk, bool isLast) {
                if (token->aborted.load(std::memory_order_acquire)) return;
                ctx->body.append(chunk.data(), chunk.length());
                if (isLast) {
                    er_http_on_fallback(rust_server, token, &ctx->view,
                                        ctx->method.data(), ctx->method.length(),
                                        ctx->url.data(), ctx->url.length(),
                                        ctx->body.data(), ctx->body.length());
                }
            });
        }
    });
}

bool er_server_listen(void* server_ptr, int port) {
    if (!server_ptr) return false;
    auto* server = static_cast<ErServer*>(server_ptr);
    if (!server->app) return false;

    register_fallback_internal(server);

    void* rust_server = server->rust_server;
    bool success = false;
    server->app->listen(port, LIBUS_LISTEN_EXCLUSIVE_PORT, [&success, server, rust_server, port](auto* socket) {
        if (socket) {
            server->listen_socket = (us_listen_socket_t*)socket;
            success = true;
            er_http_on_listening_instance(rust_server, port);
            er_http_on_listening();
        } else {
            std::cerr << "[uWebSockets] Failed to listen on port " << port << std::endl;
        }
    });
    return success;
}

void er_server_run(void* server_ptr) {
    if (!server_ptr) return;
    auto* server = static_cast<ErServer*>(server_ptr);
    if (server->app) {
        server->app->run();
    }
}

void er_server_stop(void* server_ptr) {
    if (!server_ptr) return;
    auto* server = static_cast<ErServer*>(server_ptr);
    if (server->listen_socket) {
        us_listen_socket_close(server->ssl ? 1 : 0, server->listen_socket);
        server->listen_socket = nullptr;
    }
}

// ─── Legacy Wrapper APIs (Backward Compatibility) ───────────────────────────

void er_http_init() {
    if (g_default_server) {
        delete g_default_server;
    }
    g_default_server = new ErServer(false, nullptr);
    g_app = g_default_server->app;
    g_http_req_cb = nullptr;
    g_ws_open_cb = nullptr;
    g_ws_message_cb = nullptr;
    g_ws_close_cb = nullptr;
}

void er_http_init_with_callbacks(
    HttpRequestCallback http_req_cb,
    WsOpenCallback ws_open_cb,
    WsMessageCallback ws_message_cb,
    WsCloseCallback ws_close_cb
) {
    er_http_init();
    g_http_req_cb = http_req_cb;
    g_ws_open_cb = ws_open_cb;
    g_ws_message_cb = ws_message_cb;
    g_ws_close_cb = ws_close_cb;
}

void er_ws_register_route(const char* path) {
    if (g_default_server) {
        er_server_register_ws_route(g_default_server, path, 0);
    }
}

void er_ws_send(void* ws, const char* message, size_t message_len, int is_binary) {
    if (!ws || !message) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    web_socket->send(std::string_view(message, message_len), op, false);
}

void er_ws_close(void* ws) {
    if (!ws) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    web_socket->close();
}

void er_ws_close_with_code(void* ws, int code, const char* message, size_t message_len) {
    if (!ws) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    if (code > 0 || message_len > 0) {
        web_socket->end(code, std::string_view(message ? message : "", message_len));
    } else {
        web_socket->close();
    }
}

bool er_ws_subscribe(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->subscribe(std::string_view(topic, topic_len));
}

bool er_ws_unsubscribe(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->unsubscribe(std::string_view(topic, topic_len));
}

bool er_ws_is_subscribed(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->isSubscribed(std::string_view(topic, topic_len));
}

bool er_ws_publish(void* ws, const char* topic, size_t topic_len, const char* message, size_t message_len, int is_binary) {
    if (!ws || !topic || !message) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    return web_socket->publish(std::string_view(topic, topic_len), std::string_view(message, message_len), op);
}

bool er_app_publish(const char* topic, size_t topic_len, const char* message, size_t message_len, int is_binary) {
    if (!g_default_server || !g_default_server->app || !topic || !message) return false;
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    return g_default_server->app->publish(std::string_view(topic, topic_len), std::string_view(message, message_len), op);
}

unsigned int er_app_num_subscribers(const char* topic, size_t topic_len) {
    if (!g_default_server || !g_default_server->app || !topic) return 0;
    return g_default_server->app->numSubscribers(std::string_view(topic, topic_len));
}

void er_http_register_route(const char* method, const char* path) {
    if (g_default_server) {
        er_server_register_route(g_default_server, method, path, 0);
    }
}

void er_http_listen_and_run(int port) {
    if (!g_default_server) return;
    if (er_server_listen(g_default_server, port)) {
        er_server_run(g_default_server);
    }
}

// ─── Response Handling ───────────────────────────────────────────────────────

bool er_http_response_is_alive(void* token_ptr) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    return !token->aborted.load(std::memory_order_acquire) && !token->responded.load(std::memory_order_acquire);
}

void er_http_response_release(void* token_ptr) {
    if (!token_ptr) return;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    token->release();
}

bool er_http_response_end_json(void* token_ptr, const char* json_str, size_t json_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    bool expected = false;
    if (!token->responded.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return false;
    }
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeHeader("Content-Type", "application/json");
    http_res->end(std::string_view(json_str, json_len));
    return true;
}

bool er_http_response_end_html(void* token_ptr, const char* html_str, size_t html_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    bool expected = false;
    if (!token->responded.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return false;
    }
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeHeader("Content-Type", "text/html; charset=utf-8");
    http_res->end(std::string_view(html_str, html_len));
    return true;
}

bool er_http_response_write_status(void* token_ptr, const char* status_str, size_t status_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    if (token->responded.load(std::memory_order_acquire)) return false;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeStatus(std::string_view(status_str, status_len));
    return true;
}

bool er_http_response_write_header(void* token_ptr, const char* key_str, size_t key_len, const char* val_str, size_t val_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    if (token->responded.load(std::memory_order_acquire)) return false;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeHeader(std::string_view(key_str, key_len), std::string_view(val_str, val_len));
    return true;
}

bool er_http_response_end(void* token_ptr, const char* data_str, size_t data_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    bool expected = false;
    if (!token->responded.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return false;
    }
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->end(std::string_view(data_str, data_len));
    return true;
}

bool er_http_response_write(void* token_ptr, const char* data_str, size_t data_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    if (token->responded.load(std::memory_order_acquire)) return false;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    return http_res->write(std::string_view(data_str, data_len));
}

struct AsyncSocketAccessor : public uWS::AsyncSocket<false> {
    using uWS::AsyncSocket<false>::getBufferedAmount;
};

size_t er_http_response_get_buffered_amount(void* token_ptr) {
    if (!token_ptr) return 0;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return 0;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return 0;
    return static_cast<AsyncSocketAccessor*>(static_cast<void*>(http_res))->getBufferedAmount();
}

size_t er_ws_get_buffered_amount(void* ws) {
    if (!ws) return 0;
    auto* websocket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return websocket->getBufferedAmount();
}

void er_http_create_timer(int ms, void (*cb)(void*)) {
    auto* loop = uWS::Loop::get();
    struct us_timer_t *timer = us_create_timer((struct us_loop_t *) loop, 0, sizeof(void (*)(void*)));
    std::memcpy(us_timer_ext(timer), &cb, sizeof(void (*)(void*)));
    us_timer_set(timer, [](struct us_timer_t *t) {
        void (*cb)(void*);
        std::memcpy(&cb, us_timer_ext(t), sizeof(void (*)(void*)));
        cb(t);
    }, ms, ms);
}

// ─── Cranelift Native AOT HTTP Bridge ───────────────────────────────────────

constexpr uint32_t ER_CLIF_MAGIC_REQ = 0x45525251; // 'ERRQ'
constexpr uint32_t ER_CLIF_MAGIC_OBJ = 0x45524F42; // 'EROB'

struct ErClifReq {
    uint32_t magic;
    std::string path;
    std::string method;
    std::string url;
    std::string body;
    void* token;
};

const char* er_clif_req_get_path(void* req_ptr) {
    if (!req_ptr) return "";
    return static_cast<ErClifReq*>(req_ptr)->path.c_str();
}

const char* er_clif_req_get_method(void* req_ptr) {
    if (!req_ptr) return "";
    return static_cast<ErClifReq*>(req_ptr)->method.c_str();
}

const char* er_clif_req_get_url(void* req_ptr) {
    if (!req_ptr) return "";
    return static_cast<ErClifReq*>(req_ptr)->url.c_str();
}

const char* er_clif_req_get_body(void* req_ptr) {
    if (!req_ptr) return "";
    return static_cast<ErClifReq*>(req_ptr)->body.c_str();
}

struct ErClifProp {
    char key[64];
    int64_t value;
};

struct ErClifObject {
    uint32_t magic;
    uint32_t count;
    uint32_t cap;
    ErClifProp props[64];
};

const char* er_clif_as_str(int64_t val) {
    if (!val) return "";
    uint64_t u = static_cast<uint64_t>(val);
    if ((u & 0xffff000000000000ULL) == 0xfff4000000000000ULL) {
        return reinterpret_cast<const char*>(u & 0x0000ffffffffffffULL);
    }
    if ((u & 0xffff000000000000ULL) == 0) {
        return reinterpret_cast<const char*>(u);
    }
    return "";
}

int64_t er_clif_alloc_object() {
    auto* obj = new ErClifObject();
    obj->magic = ER_CLIF_MAGIC_OBJ;
    obj->count = 0;
    obj->cap = 64;
    return reinterpret_cast<int64_t>(obj);
}

void er_clif_object_set(int64_t obj_val, int64_t key_val, int64_t val) {
    if (!obj_val) return;
    auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
    const char* key = er_clif_as_str(key_val);
    if (!key || obj->magic != ER_CLIF_MAGIC_OBJ || obj->count >= 64) return;
    for (uint32_t i = 0; i < obj->count; ++i) {
        if (strcmp(obj->props[i].key, key) == 0) {
            obj->props[i].value = val;
            return;
        }
    }
    strncpy(obj->props[obj->count].key, key, 63);
    obj->props[obj->count].key[63] = '\0';
    obj->props[obj->count].value = val;
    obj->count++;
}

int64_t er_clif_object_get(int64_t obj_val, int64_t key_val) {
    if (!obj_val) return 0;
    const char* key = er_clif_as_str(key_val);
    if (!key) return 0;

    uint32_t magic = *reinterpret_cast<const uint32_t*>(obj_val);
    if (magic == ER_CLIF_MAGIC_REQ) {
        auto* req = reinterpret_cast<ErClifReq*>(obj_val);
        if (strcmp(key, "path") == 0) {
            return reinterpret_cast<int64_t>(req->path.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "method") == 0) {
            return reinterpret_cast<int64_t>(req->method.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "url") == 0) {
            return reinterpret_cast<int64_t>(req->url.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "body") == 0) {
            return reinterpret_cast<int64_t>(req->body.c_str()) | 0xfff4000000000000ULL;
        }
        return 0;
    }

    if (magic == ER_CLIF_MAGIC_OBJ) {
        auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
        for (uint32_t i = 0; i < obj->count; ++i) {
            if (strcmp(obj->props[i].key, key) == 0) {
                return obj->props[i].value;
            }
        }
    }
    return 0;
}

int64_t er_clif_object_get_str(int64_t obj_val, const char* key) {
    if (!obj_val || !key) return 0;
    uint32_t magic = *reinterpret_cast<const uint32_t*>(obj_val);
    if (magic == ER_CLIF_MAGIC_REQ) {
        auto* req = reinterpret_cast<ErClifReq*>(obj_val);
        if (strcmp(key, "path") == 0) {
            return reinterpret_cast<int64_t>(req->path.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "method") == 0) {
            return reinterpret_cast<int64_t>(req->method.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "url") == 0) {
            return reinterpret_cast<int64_t>(req->url.c_str()) | 0xfff4000000000000ULL;
        }
        if (strcmp(key, "body") == 0) {
            return reinterpret_cast<int64_t>(req->body.c_str()) | 0xfff4000000000000ULL;
        }
        return 0;
    }
    if (magic == ER_CLIF_MAGIC_OBJ) {
        auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
        for (uint32_t i = 0; i < obj->count; ++i) {
            if (strcmp(obj->props[i].key, key) == 0) {
                return obj->props[i].value;
            }
        }
    }
    return 0;
}

int64_t er_clif_str_eq(int64_t a, int64_t b) {
    if (a == b) return 1;
    const char* sa = er_clif_as_str(a);
    const char* sb = er_clif_as_str(b);
    if (!sa || !sb) return 0;
    return strcmp(sa, sb) == 0 ? 1 : 0;
}

void er_clif_json_serialize_value(int64_t val, std::string& out) {
    if (!val) {
        out += "null";
        return;
    }
    uint64_t u = static_cast<uint64_t>(val);
    if ((u & 0xffff000000000000ULL) == 0xfff4000000000000ULL) {
        const char* s = reinterpret_cast<const char*>(u & 0x0000ffffffffffffULL);
        out += "\"";
        if (s) {
            for (const char* p = s; *p; ++p) {
                if (*p == '"') out += "\\\"";
                else if (*p == '\\') out += "\\\\";
                else if (*p == '\n') out += "\\n";
                else if (*p == '\r') out += "\\r";
                else if (*p == '\t') out += "\\t";
                else out += *p;
            }
        }
        out += "\"";
        return;
    }

    uint32_t magic = 0;
    if (u > 0x1000 && u < 0x0000ffffffffffffULL) {
        magic = *reinterpret_cast<const uint32_t*>(val);
    }

    if (magic == ER_CLIF_MAGIC_OBJ) {
        auto* obj = reinterpret_cast<ErClifObject*>(val);
        out += "{";
        for (uint32_t i = 0; i < obj->count; ++i) {
            if (i > 0) out += ",";
            out += "\"";
            out += obj->props[i].key;
            out += "\":";
            er_clif_json_serialize_value(obj->props[i].value, out);
        }
        out += "}";
        return;
    } else if (magic == ER_CLIF_MAGIC_REQ) {
        auto* req = reinterpret_cast<ErClifReq*>(val);
        out += "{\"path\":\"" + req->path + "\",\"method\":\"" + req->method + "\",\"url\":\"" + req->url + "\"}";
        return;
    }

    out += std::to_string(val);
}

int64_t er_clif_json_stringify(int64_t val) {
    std::string buf;
    er_clif_json_serialize_value(val, buf);
    char* copy = strdup(buf.c_str());
    return reinterpret_cast<int64_t>(copy) | 0xfff4000000000000ULL;
}

int64_t er_clif_str_concat(int64_t a, int64_t b) {
    const char* sa = er_clif_as_str(a);
    const char* sb = er_clif_as_str(b);
    std::string res;
    if (sa && *sa) {
        res += sa;
    } else if (a != 0 && (static_cast<uint64_t>(a) & 0xffff000000000000ULL) != 0xfff4000000000000ULL) {
        res += std::to_string(a);
    }
    if (sb && *sb) {
        res += sb;
    } else if (b != 0 && (static_cast<uint64_t>(b) & 0xffff000000000000ULL) != 0xfff4000000000000ULL) {
        res += std::to_string(b);
    }
    char* copy = strdup(res.c_str());
    return reinterpret_cast<int64_t>(copy) | 0xfff4000000000000ULL;
}

static thread_local HttpResponseToken* g_clif_token = nullptr;

void er_clif_respond(int32_t status, const char* content_type, const char* body, size_t body_len) {
    if (!g_clif_token) return;
    auto* token = g_clif_token;
    if (token->aborted.load(std::memory_order_acquire)) return;
    if (token->responded.exchange(true, std::memory_order_acq_rel)) return;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return;

    std::string status_str;
    switch (status) {
        case 200: status_str = "200 OK"; break;
        case 201: status_str = "201 Created"; break;
        case 204: status_str = "204 No Content"; break;
        case 400: status_str = "400 Bad Request"; break;
        case 404: status_str = "404 Not Found"; break;
        case 500: status_str = "500 Internal Server Error"; break;
        default: status_str = std::to_string(status) + " OK"; break;
    }

    http_res->writeStatus(status_str);
    if (content_type && strlen(content_type) > 0) {
        http_res->writeHeader("content-type", content_type);
    }
    std::string_view payload(body ? body : "", body ? body_len : 0);
    http_res->end(payload);
}

typedef int64_t (*ErClifFetchCallback)(void* req);

void er_clif_serve(int32_t port, ErClifFetchCallback fetch_fn) {
    if (port <= 0) port = 3000;
    auto app = uWS::App();
    app.any("/*", [fetch_fn](auto* res, auto* req) {
        auto* token = new HttpResponseToken(res);
        res->onAborted(AbortHandler(token));

        ErClifReq clif_req;
        clif_req.magic = ER_CLIF_MAGIC_REQ;
        clif_req.path = std::string(req->getUrl());
        clif_req.method = std::string(req->getCaseSensitiveMethod());
        clif_req.url = std::string(req->getFullUrl());
        clif_req.token = token;

        g_clif_token = token;
        int64_t ret = fetch_fn ? fetch_fn(&clif_req) : 0;

        if (!token->aborted.load(std::memory_order_acquire) && !token->responded.load(std::memory_order_acquire)) {
            if (ret != 0) {
                int64_t is_resp = er_clif_object_get_str(ret, "_isResponse");
                if (is_resp) {
                    int64_t status = er_clif_object_get_str(ret, "status");
                    if (status <= 0) status = 200;
                    int64_t body_val = er_clif_object_get_str(ret, "body");
                    const char* body_str = er_clif_as_str(body_val);
                    int64_t headers_val = er_clif_object_get_str(ret, "headers");
                    const char* content_type = "text/plain; charset=utf-8";
                    if (headers_val) {
                        int64_t ct_val = er_clif_object_get_str(headers_val, "content-type");
                        const char* ct_str = er_clif_as_str(ct_val);
                        if (ct_str && strlen(ct_str) > 0) content_type = ct_str;
                    }
                    er_clif_respond((int32_t)status, content_type, body_str, body_str ? strlen(body_str) : 0);
                } else {
                    std::string json_body;
                    er_clif_json_serialize_value(ret, json_body);
                    er_clif_respond(200, "application/json; charset=utf-8", json_body.c_str(), json_body.size());
                }
            } else {
                er_clif_respond(200, "text/plain; charset=utf-8", "OK", 2);
            }
        }
        g_clif_token = nullptr;
    });

    app.listen(port, [port](auto* listen_socket) {
        if (listen_socket) {
            std::cout << "[HTTP] Server listening on http://0.0.0.0:" << port << std::endl;
        } else {
            std::cerr << "[HTTP] Failed to listen on port " << port << std::endl;
        }
    });

    app.run();
}

void er_clif_serve_config(int64_t config_val) {
    int32_t port = 3000;
    int64_t port_val = er_clif_object_get_str(config_val, "port");
    if (port_val > 0 && port_val <= 65535) {
        port = static_cast<int32_t>(port_val);
    }
    int64_t fetch_fn_val = er_clif_object_get_str(config_val, "fetch");
    ErClifFetchCallback fetch_fn = reinterpret_cast<ErClifFetchCallback>(fetch_fn_val);
    er_clif_serve(port, fetch_fn);
}

} // extern "C"
