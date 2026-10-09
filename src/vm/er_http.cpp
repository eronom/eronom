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

extern "C" {
    // Legacy callbacks
    void er_http_on_request(void* res, const char* method, size_t method_len, const char* path, size_t path_len, const char* headers, size_t headers_len, const char* body, size_t body_len);
    void er_ws_on_open(void* ws, const char* path, size_t path_len);
    void er_ws_on_message(void* ws, const char* path, size_t path_len, const char* message, size_t message_len, int is_binary);
    void er_ws_on_close(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);
    void er_http_on_listening();

    // Direct dispatch callbacks
    void er_http_on_route(
        void* rust_server,
        uint32_t route_id,
        void* token,
        void* req,
        const char* url,
        size_t url_len,
        const char* body,
        size_t body_len
    );

    void er_http_on_fallback(
        void* rust_server,
        void* token,
        void* req,
        const char* method,
        size_t method_len,
        const char* url,
        size_t url_len,
        const char* body,
        size_t body_len
    );

    void er_http_on_listening_instance(void* rust_server, int port);
    void er_ws_on_open_instance(void* rust_server, uint32_t route_id, void* ws, const char* path, size_t path_len);
    void er_ws_on_message_instance(void* rust_server, uint32_t route_id, void* ws, const char* path, size_t path_len, const char* message, size_t message_len, int is_binary);
    void er_ws_on_close_instance(void* rust_server, uint32_t route_id, void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);
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

} // extern "C"
