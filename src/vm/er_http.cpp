#include "App.h"
#include <string>
#include <string_view>
#include <iostream>
#include <cstring>
#include <memory>
#include <atomic>
#include <cstdint>

extern "C" {
    void er_http_on_request(void* res, const char* method, size_t method_len, const char* path, size_t path_len, const char* headers, size_t headers_len, const char* body, size_t body_len);
    void er_ws_on_open(void* ws, const char* path, size_t path_len);
    void er_ws_on_message(void* ws, const char* path, size_t path_len, const char* message, size_t message_len, int is_binary);
    void er_ws_on_close(void* ws, const char* path, size_t path_len, int code, const char* message, size_t message_len);
    void er_http_on_listening();
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
    // Fill with user data if needed
};

struct HttpResponseToken {
    std::atomic<uWS::HttpResponse<false>*> res;
    std::atomic<bool> aborted{false};
    std::atomic<bool> responded{false};
    std::atomic<uint32_t> ref_count{2}; // 1 for uWS onAborted wrapper, 1 for Rust/VM context

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
    
    uWS::App::WebSocketBehavior<PerSocketData> ws_behavior;
    ws_behavior.compression = uWS::CompressOptions(uWS::SHARED_COMPRESSOR);
    ws_behavior.maxPayloadLength = 16 * 1024 * 1024;
    ws_behavior.idleTimeout = 120;
    ws_behavior.maxBackpressure = 16 * 1024 * 1024;
    ws_behavior.closeOnBackpressureLimit = false;
    ws_behavior.resetIdleTimeoutOnSend = false;
    ws_behavior.sendPingsAutomatically = true;
    ws_behavior.open = [path_str](auto* ws) {
        if (g_ws_open_cb) {
            g_ws_open_cb(ws, path_str.data(), path_str.length());
        } else {
            er_ws_on_open(ws, path_str.data(), path_str.length());
        }
    };
    ws_behavior.message = [path_str](auto* ws, std::string_view message, uWS::OpCode opCode) {
        int is_binary = (opCode == uWS::OpCode::BINARY) ? 1 : 0;
        if (g_ws_message_cb) {
            g_ws_message_cb(ws, path_str.data(), path_str.length(), message.data(), message.length(), is_binary);
        } else {
            er_ws_on_message(ws, path_str.data(), path_str.length(), message.data(), message.length(), is_binary);
        }
    };
    ws_behavior.close = [path_str](auto* ws, int code, std::string_view message) {
        if (g_ws_close_cb) {
            g_ws_close_cb(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
        } else {
            er_ws_on_close(ws, path_str.data(), path_str.length(), code, message.data(), message.length());
        }
    };

    g_app->ws<PerSocketData>(path_str, std::move(ws_behavior));
}

extern "C" void er_ws_send(void* ws, const char* message, size_t message_len, int is_binary) {
    if (!ws || !message) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    web_socket->send(std::string_view(message, message_len), op, false);
}

extern "C" void er_ws_close(void* ws) {
    if (!ws) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    web_socket->close();
}

extern "C" void er_ws_close_with_code(void* ws, int code, const char* message, size_t message_len) {
    if (!ws) return;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    if (code > 0 || message_len > 0) {
        web_socket->end(code, std::string_view(message ? message : "", message_len));
    } else {
        web_socket->close();
    }
}

extern "C" bool er_ws_subscribe(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->subscribe(std::string_view(topic, topic_len));
}

extern "C" bool er_ws_unsubscribe(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->unsubscribe(std::string_view(topic, topic_len));
}

extern "C" bool er_ws_is_subscribed(void* ws, const char* topic, size_t topic_len) {
    if (!ws || !topic) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    return web_socket->isSubscribed(std::string_view(topic, topic_len));
}

extern "C" bool er_ws_publish(void* ws, const char* topic, size_t topic_len, const char* message, size_t message_len, int is_binary) {
    if (!ws || !topic || !message) return false;
    auto* web_socket = static_cast<uWS::WebSocket<false, true, PerSocketData>*>(ws);
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    return web_socket->publish(std::string_view(topic, topic_len), std::string_view(message, message_len), op);
}

extern "C" bool er_app_publish(const char* topic, size_t topic_len, const char* message, size_t message_len, int is_binary) {
    if (!g_app || !topic || !message) return false;
    uWS::OpCode op = (is_binary != 0) ? uWS::OpCode::BINARY : uWS::OpCode::TEXT;
    return g_app->publish(std::string_view(topic, topic_len), std::string_view(message, message_len), op);
}

extern "C" unsigned int er_app_num_subscribers(const char* topic, size_t topic_len) {
    if (!g_app || !topic) return 0;
    return g_app->numSubscribers(std::string_view(topic, topic_len));
}

extern "C" void er_http_register_route(const char* method, const char* path) {
    if (!g_app) return;
    
    std::string method_str(method);
    std::string path_str(path);
    for (char &c : method_str) {
        c = (char)toupper((unsigned char)c);
    }
    
    if (method_str == "GET") {
        g_app->get(path_str, [](auto* res, auto* req) {
            std::string_view method = "GET";
            std::string full_url = std::string(req->getUrl());
            std::string_view query = req->getQuery();
            if (!query.empty()) {
                full_url.push_back('?');
                full_url.append(query);
            }
            
            std::string headers_str;
            for (auto h : *req) {
                headers_str.append(h.first);
                headers_str.append(": ");
                headers_str.append(h.second);
                headers_str.append("\r\n");
            }
            
            auto* token = new HttpResponseToken(res);
            res->onAborted(AbortHandler(token));
            if (g_http_req_cb) {
                g_http_req_cb(token, method.data(), method.length(), full_url.data(), full_url.length(),
                              headers_str.data(), headers_str.length(), nullptr, 0);
            } else {
                er_http_on_request(token, method.data(), method.length(), full_url.data(), full_url.length(),
                                   headers_str.data(), headers_str.length(), nullptr, 0);
            }
        });
    } else if (method_str == "HEAD") {
        g_app->head(path_str, [](auto* res, auto* req) {
            std::string_view method = "HEAD";
            std::string full_url = std::string(req->getUrl());
            std::string_view query = req->getQuery();
            if (!query.empty()) {
                full_url.push_back('?');
                full_url.append(query);
            }
            
            std::string headers_str;
            for (auto h : *req) {
                headers_str.append(h.first);
                headers_str.append(": ");
                headers_str.append(h.second);
                headers_str.append("\r\n");
            }
            
            auto* token = new HttpResponseToken(res);
            res->onAborted(AbortHandler(token));
            if (g_http_req_cb) {
                g_http_req_cb(token, method.data(), method.length(), full_url.data(), full_url.length(),
                              headers_str.data(), headers_str.length(), nullptr, 0);
            } else {
                er_http_on_request(token, method.data(), method.length(), full_url.data(), full_url.length(),
                                   headers_str.data(), headers_str.length(), nullptr, 0);
            }
        });
    } else {
        auto register_body_handler = [&](auto attach_fn, const char* default_verb) {
            attach_fn(path_str, [default_verb](auto* res, auto* req) {
                std::string method_name = default_verb ? std::string(default_verb) : "";
                if (method_name.empty()) {
                    std::string_view req_m = req->getCaseSensitiveMethod();
                    method_name = std::string(req_m);
                    for (char &c : method_name) {
                        c = (char)toupper((unsigned char)c);
                    }
                }
                std::string full_url = std::string(req->getUrl());
                std::string_view query = req->getQuery();
                if (!query.empty()) {
                    full_url.push_back('?');
                    full_url.append(query);
                }
                
                std::string headers_str;
                for (auto h : *req) {
                    headers_str.append(h.first);
                    headers_str.append(": ");
                    headers_str.append(h.second);
                    headers_str.append("\r\n");
                }
                
                auto* token = new HttpResponseToken(res);
                res->onAborted(AbortHandler(token));

                struct ReqCtx {
                    HttpResponseToken* token;
                    std::string method;
                    std::string path;
                    std::string headers;
                    std::string body;
                    ReqCtx(HttpResponseToken* t) : token(t) {
                        if (token) token->add_ref();
                    }
                    ~ReqCtx() {
                        if (token) {
                            token->release();
                            token = nullptr;
                        }
                    }
                };
                auto ctx = std::make_shared<ReqCtx>(token);
                ctx->method = std::move(method_name);
                ctx->path = std::move(full_url);
                ctx->headers = std::move(headers_str);
                
                res->onData([ctx, token](std::string_view chunk, bool isLast) {
                    if (token->aborted.load(std::memory_order_acquire)) return;
                    ctx->body.append(chunk.data(), chunk.length());
                    if (isLast) {
                        if (g_http_req_cb) {
                            g_http_req_cb(token, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                          ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                        } else {
                            er_http_on_request(token, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                               ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                        }
                    }
                });
            });
        };

        if (method_str == "POST") {
            register_body_handler([](const std::string& p, auto h) { g_app->post(p, std::move(h)); }, "POST");
        } else if (method_str == "PUT") {
            register_body_handler([](const std::string& p, auto h) { g_app->put(p, std::move(h)); }, "PUT");
        } else if (method_str == "PATCH") {
            register_body_handler([](const std::string& p, auto h) { g_app->patch(p, std::move(h)); }, "PATCH");
        } else if (method_str == "DELETE" || method_str == "DEL") {
            register_body_handler([](const std::string& p, auto h) { g_app->del(p, std::move(h)); }, "DELETE");
        } else if (method_str == "OPTIONS") {
            register_body_handler([](const std::string& p, auto h) { g_app->options(p, std::move(h)); }, "OPTIONS");
        } else if (method_str == "ALL" || method_str == "ANY" || method_str == "*") {
            register_body_handler([](const std::string& p, auto h) { g_app->any(p, std::move(h)); }, nullptr);
        }
    }
}

extern "C" void er_http_listen_and_run(int port) {
    if (!g_app) return;
    
    // Register wildcard fallback on any method to catch any unhandled request and forward to er_http_on_request
    g_app->any("/*", [](auto* res, auto* req) {
        std::string_view req_m = req->getCaseSensitiveMethod();
        std::string method_str = std::string(req_m);
        for (char &c : method_str) {
            c = (char)toupper((unsigned char)c);
        }
        std::string full_url = std::string(req->getUrl());
        std::string_view query = req->getQuery();
        if (!query.empty()) {
            full_url.push_back('?');
            full_url.append(query);
        }
        
        std::string headers_str;
        for (auto h : *req) {
            headers_str.append(h.first);
            headers_str.append(": ");
            headers_str.append(h.second);
            headers_str.append("\r\n");
        }
        
        auto* token = new HttpResponseToken(res);
        res->onAborted(AbortHandler(token));

        struct FallbackCtx {
            HttpResponseToken* token;
            std::string method;
            std::string path;
            std::string headers;
            std::string body;
            FallbackCtx(HttpResponseToken* t) : token(t) {
                if (token) token->add_ref();
            }
            ~FallbackCtx() {
                if (token) {
                    token->release();
                    token = nullptr;
                }
            }
        };
        auto ctx = std::make_shared<FallbackCtx>(token);
        ctx->method = std::move(method_str);
        ctx->path = std::move(full_url);
        ctx->headers = std::move(headers_str);
        
        res->onData([ctx, token](std::string_view chunk, bool isLast) {
            if (token->aborted.load(std::memory_order_acquire)) return;
            ctx->body.append(chunk.data(), chunk.length());
            if (isLast) {
                if (g_http_req_cb) {
                    g_http_req_cb(token, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
                                  ctx->headers.data(), ctx->headers.length(), ctx->body.data(), ctx->body.length());
                } else {
                    er_http_on_request(token, ctx->method.data(), ctx->method.length(), ctx->path.data(), ctx->path.length(),
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

extern "C" bool er_http_response_is_alive(void* token_ptr) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    return !token->aborted.load(std::memory_order_acquire) && !token->responded.load(std::memory_order_acquire);
}

extern "C" void er_http_response_release(void* token_ptr) {
    if (!token_ptr) return;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    token->release();
}

extern "C" bool er_http_response_end_json(void* token_ptr, const char* json_str, size_t json_len) {
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

extern "C" bool er_http_response_end_html(void* token_ptr, const char* html_str, size_t html_len) {
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

extern "C" bool er_http_response_write_status(void* token_ptr, const char* status_str, size_t status_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeStatus(std::string_view(status_str, status_len));
    return true;
}

extern "C" bool er_http_response_write_header(void* token_ptr, const char* key_str, size_t key_len, const char* val_str, size_t val_len) {
    if (!token_ptr) return false;
    auto* token = static_cast<HttpResponseToken*>(token_ptr);
    if (token->aborted.load(std::memory_order_acquire)) return false;
    auto* http_res = token->res.load(std::memory_order_acquire);
    if (!http_res) return false;
    http_res->writeHeader(std::string_view(key_str, key_len), std::string_view(val_str, val_len));
    return true;
}

extern "C" bool er_http_response_end(void* token_ptr, const char* data_str, size_t data_len) {
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



