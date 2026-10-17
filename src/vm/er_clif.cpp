#include "App.h"
#include <iostream>
#include <string>
#include <vector>
#include <functional>
#include <cstring>
#include <cstdint>
#include <atomic>

// ─── Cranelift Native AOT HTTP Engine ───────────────────────────────────────

constexpr uint32_t ER_CLIF_MAGIC_REQ = 0x45525251; // 'ERRQ'
constexpr uint32_t ER_CLIF_MAGIC_OBJ = 0x45524F42; // 'EROB'

struct ErClifResponseToken {
    std::atomic<uWS::HttpResponse<false>*> res;
    std::atomic<bool> aborted{false};
    std::atomic<bool> responded{false};
    std::atomic<uint32_t> ref_count{2}; // 1 for uWS onAborted wrapper, 1 for active callback

    ErClifResponseToken(uWS::HttpResponse<false>* r) : res(r) {}

    void add_ref() {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    void release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            delete this;
        }
    }
};

struct ErClifAbortHandler {
    ErClifResponseToken* token;
    explicit ErClifAbortHandler(ErClifResponseToken* t) : token(t) {}
    ErClifAbortHandler(const ErClifAbortHandler& o) : token(o.token) {
        if (token) token->add_ref();
    }
    ErClifAbortHandler(ErClifAbortHandler&& o) noexcept : token(o.token) {
        o.token = nullptr;
    }
    ErClifAbortHandler& operator=(const ErClifAbortHandler& o) {
        if (this != &o) {
            if (token) token->release();
            token = o.token;
            if (token) token->add_ref();
        }
        return *this;
    }
    ErClifAbortHandler& operator=(ErClifAbortHandler&& o) noexcept {
        if (this != &o) {
            if (token) token->release();
            token = o.token;
            o.token = nullptr;
        }
        return *this;
    }
    ~ErClifAbortHandler() {
        if (token) token->release();
    }
    void operator()() {
        if (token) {
            token->aborted.store(true, std::memory_order_release);
            token->res.store(nullptr, std::memory_order_release);
        }
    }
};

struct ErClifReq {
    uint32_t magic;
    std::string path;
    std::string method;
    std::string url;
    std::string body;
    ErClifResponseToken* token;
};

extern "C" {

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
    auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
    if (!obj || obj->magic != ER_CLIF_MAGIC_OBJ) return;
    const char* key = er_clif_as_str(key_val);
    if (!key) return;
    for (uint32_t i = 0; i < obj->count; ++i) {
        if (std::strcmp(obj->props[i].key, key) == 0) {
            obj->props[i].value = val;
            return;
        }
    }
    if (obj->count < obj->cap) {
        std::strncpy(obj->props[obj->count].key, key, 63);
        obj->props[obj->count].key[63] = '\0';
        obj->props[obj->count].value = val;
        obj->count++;
    }
}

int64_t er_clif_object_get(int64_t obj_val, int64_t key_val) {
    auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
    const char* key = er_clif_as_str(key_val);
    if (!obj || !key) return 0;
    if (obj->magic == ER_CLIF_MAGIC_REQ) {
        auto* req = reinterpret_cast<ErClifReq*>(obj_val);
        if (std::strcmp(key, "path") == 0) {
            return reinterpret_cast<int64_t>(req->path.c_str());
        }
        if (std::strcmp(key, "method") == 0) {
            return reinterpret_cast<int64_t>(req->method.c_str());
        }
        if (std::strcmp(key, "url") == 0) {
            return reinterpret_cast<int64_t>(req->url.c_str());
        }
        if (std::strcmp(key, "body") == 0) {
            return reinterpret_cast<int64_t>(req->body.c_str());
        }
        return 0;
    }
    if (obj->magic != ER_CLIF_MAGIC_OBJ) return 0;
    for (uint32_t i = 0; i < obj->count; ++i) {
        if (std::strcmp(obj->props[i].key, key) == 0) {
            return obj->props[i].value;
        }
    }
    return 0;
}

int64_t er_clif_object_get_str(int64_t obj_val, const char* key) {
    auto* obj = reinterpret_cast<ErClifObject*>(obj_val);
    if (!obj || !key) return 0;
    if (obj->magic == ER_CLIF_MAGIC_REQ) {
        auto* req = reinterpret_cast<ErClifReq*>(obj_val);
        if (std::strcmp(key, "path") == 0) {
            return reinterpret_cast<int64_t>(req->path.c_str());
        }
        if (std::strcmp(key, "method") == 0) {
            return reinterpret_cast<int64_t>(req->method.c_str());
        }
        if (std::strcmp(key, "url") == 0) {
            return reinterpret_cast<int64_t>(req->url.c_str());
        }
        if (std::strcmp(key, "body") == 0) {
            return reinterpret_cast<int64_t>(req->body.c_str());
        }
        return 0;
    }
    if (obj->magic != ER_CLIF_MAGIC_OBJ) return 0;
    for (uint32_t i = 0; i < obj->count; ++i) {
        if (std::strcmp(obj->props[i].key, key) == 0) {
            return obj->props[i].value;
        }
    }
    return 0;
}

int64_t er_clif_str_eq(int64_t a, int64_t b) {
    if (a == b) return 1;
    const char* sa = er_clif_as_str(a);
    const char* sb = er_clif_as_str(b);
    if (sa && sb && std::strcmp(sa, sb) == 0) return 1;
    return 0;
}

void er_clif_json_serialize_value(int64_t val, std::string& out) {
    const char* s = er_clif_as_str(val);
    if (s && *s) {
        out += '"';
        for (const char* p = s; *p; ++p) {
            if (*p == '"') out += "\\\"";
            else if (*p == '\\') out += "\\\\";
            else if (*p == '\n') out += "\\n";
            else if (*p == '\r') out += "\\r";
            else if (*p == '\t') out += "\\t";
            else out += *p;
        }
        out += '"';
        return;
    }
    auto* obj = reinterpret_cast<ErClifObject*>(val);
    if (obj && obj->magic == ER_CLIF_MAGIC_OBJ) {
        out += '{';
        for (uint32_t i = 0; i < obj->count; ++i) {
            if (i > 0) out += ',';
            out += '"';
            out += obj->props[i].key;
            out += "\":";
            er_clif_json_serialize_value(obj->props[i].value, out);
        }
        out += '}';
        return;
    }
    if ((val & 0xffff000000000000ULL) == 0) {
        out += std::to_string(val);
        return;
    }
    out += "null";
}

int64_t er_clif_json_stringify(int64_t val) {
    std::string buf;
    er_clif_json_serialize_value(val, buf);
    char* res = new char[buf.size() + 1];
    std::memcpy(res, buf.c_str(), buf.size() + 1);
    return reinterpret_cast<int64_t>(res);
}

int64_t er_clif_str_concat(int64_t a, int64_t b) {
    const char* sa = er_clif_as_str(a);
    const char* sb = er_clif_as_str(b);
    size_t la = sa ? std::strlen(sa) : 0;
    size_t lb = sb ? std::strlen(sb) : 0;
    char* res = new char[la + lb + 1];
    if (la > 0) std::memcpy(res, sa, la);
    if (lb > 0) std::memcpy(res + la, sb, lb);
    res[la + lb] = '\0';
    return reinterpret_cast<int64_t>(res);
}

typedef int64_t (*ErClifFetchCallback)(void* req_ptr);

static ErClifResponseToken* g_clif_token = nullptr;

void er_clif_respond(int32_t status, const char* content_type, const char* body, size_t body_len) {
    if (!g_clif_token) return;
    auto* token = g_clif_token;
    if (token->aborted.load(std::memory_order_acquire)) return;
    bool expected = false;
    if (!token->responded.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }
    auto* res = token->res.load(std::memory_order_acquire);
    if (!res) return;

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

    res->writeStatus(status_str);
    if (content_type && std::strlen(content_type) > 0) {
        res->writeHeader("content-type", content_type);
    }
    res->end(std::string_view(body ? body : "", body_len));
}

void er_clif_serve(int32_t port, ErClifFetchCallback fetch_fn) {
    if (port <= 0) port = 3000;
    auto app = uWS::App();
    app.any("/*", [fetch_fn](auto* res, auto* req) {
        auto* token = new ErClifResponseToken(res);
        res->onAborted(ErClifAbortHandler(token));

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
                        if (ct_str && std::strlen(ct_str) > 0) content_type = ct_str;
                    }
                    er_clif_respond((int32_t)status, content_type, body_str, body_str ? std::strlen(body_str) : 0);
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
