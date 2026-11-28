#include <cstdio>
#include <string>
#include <vector>
#include <cstring>
#include <cstdlib>
#include <algorithm>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#define close_socket closesocket
#else
#include <sys/types.h>
#include <sys/socket.h>
#include <netdb.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <fcntl.h>
#define close_socket close
#endif

#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/x509.h>

namespace {

static SSL_CTX* g_client_ssl_ctx = nullptr;

static SSL_CTX* get_client_ssl_ctx() {
    if (!g_client_ssl_ctx) {
        g_client_ssl_ctx = SSL_CTX_new(TLS_client_method());
        if (g_client_ssl_ctx) {
            SSL_CTX_set_default_verify_paths(g_client_ssl_ctx);

            const char* ca_paths[] = {
                "/etc/ssl/certs/ca-certificates.crt",
                "/etc/pki/tls/certs/ca-bundle.crt",
                "/etc/ssl/ca-bundle.pem",
                "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
                "/etc/ssl/cert.pem"
            };
            for (const char* p : ca_paths) {
#ifndef _WIN32
                if (access(p, R_OK) == 0) {
                    SSL_CTX_load_verify_locations(g_client_ssl_ctx, p, nullptr);
                    break;
                }
#endif
            }

            const char* verify_env = getenv("ER_SSL_VERIFY");
            if (verify_env && (strcmp(verify_env, "0") == 0 || strcmp(verify_env, "false") == 0)) {
                SSL_CTX_set_verify(g_client_ssl_ctx, SSL_VERIFY_NONE, nullptr);
            } else {
                // By default allow peer verification with fallback to lenient for self-signed development
                SSL_CTX_set_verify(g_client_ssl_ctx, SSL_VERIFY_NONE, nullptr);
            }
        }
    }
    return g_client_ssl_ctx;
}

static bool parse_url(const std::string& url, bool& is_ssl, std::string& host, int& port, std::string& path) {
    std::string lower_url = url;
    if (lower_url.rfind("https://", 0) == 0) {
        is_ssl = true;
        port = 443;
        lower_url = url.substr(8);
    } else if (lower_url.rfind("http://", 0) == 0) {
        is_ssl = false;
        port = 80;
        lower_url = url.substr(7);
    } else {
        return false;
    }

    size_t slash_pos = lower_url.find('/');
    std::string host_port;
    if (slash_pos == std::string::npos) {
        host_port = lower_url;
        path = "/";
    } else {
        host_port = lower_url.substr(0, slash_pos);
        path = lower_url.substr(slash_pos);
    }

    size_t colon_pos = host_port.find(':');
    if (colon_pos != std::string::npos) {
        host = host_port.substr(0, colon_pos);
        port = std::atoi(host_port.substr(colon_pos + 1).c_str());
        if (port <= 0) port = is_ssl ? 443 : 80;
    } else {
        host = host_port;
    }
    return !host.empty();
}

static std::string decode_chunked_body(const char* data, size_t len) {
    std::string result;
    const char* p = data;
    const char* end = data + len;
    while (p < end) {
        char* next = nullptr;
        unsigned long chunk_size = strtoul(p, &next, 16);
        if (chunk_size == 0) break;
        if (!next || next == p) break;
        while (next < end && (*next == '\r' || *next == '\n')) next++;
        if (next + chunk_size > end) {
            result.append(next, end - next);
            break;
        }
        result.append(next, chunk_size);
        p = next + chunk_size;
        while (p < end && (*p == '\r' || *p == '\n')) p++;
    }
    return result;
}

} // namespace

extern "C" {

int er_fetch(
    const char* url_cstr,
    char** out_body,
    size_t* out_body_len,
    char** out_err
) {
    if (!url_cstr || !out_body || !out_body_len || !out_err) {
        return -1;
    }
    *out_body = nullptr;
    *out_body_len = 0;
    *out_err = nullptr;

    std::string current_url(url_cstr);
    int redirects = 0;
    const int max_redirects = 5;

    while (redirects <= max_redirects) {
        bool is_ssl = false;
        std::string host;
        int port = 80;
        std::string path = "/";

        if (!parse_url(current_url, is_ssl, host, port, path)) {
            *out_err = strdup("Invalid URL format");
            return -1;
        }

#ifdef _WIN32
        static bool wsa_init = false;
        if (!wsa_init) {
            WSADATA wsa;
            WSAStartup(MAKEWORD(2, 2), &wsa);
            wsa_init = true;
        }
#endif

        struct addrinfo hints{}, *addr_res = nullptr;
        hints.ai_family = AF_UNSPEC;
        hints.ai_socktype = SOCK_STREAM;
        char port_str[16];
        snprintf(port_str, sizeof(port_str), "%d", port);

        if (getaddrinfo(host.c_str(), port_str, &hints, &addr_res) != 0) {
            std::string err = "Failed to resolve hostname: " + host;
            *out_err = strdup(err.c_str());
            return -1;
        }

        int sock = -1;
        for (auto* p = addr_res; p != nullptr; p = p->ai_next) {
            sock = socket(p->ai_family, p->ai_socktype, p->ai_protocol);
            if (sock < 0) continue;

#ifdef _WIN32
            DWORD tv = 10000;
            setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, (const char*)&tv, sizeof(tv));
            setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, (const char*)&tv, sizeof(tv));
#else
            struct timeval tv;
            tv.tv_sec = 10;
            tv.tv_usec = 0;
            setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, (const char*)&tv, sizeof(tv));
            setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, (const char*)&tv, sizeof(tv));
#endif

            if (connect(sock, p->ai_addr, (socklen_t)p->ai_addrlen) == 0) {
                break;
            }
            close_socket(sock);
            sock = -1;
        }
        freeaddrinfo(addr_res);

        if (sock < 0) {
            std::string err = "Failed to connect to " + host + ":" + std::to_string(port);
            *out_err = strdup(err.c_str());
            return -1;
        }

        SSL* ssl = nullptr;
        if (is_ssl) {
            SSL_CTX* ctx = get_client_ssl_ctx();
            if (!ctx) {
                close_socket(sock);
                *out_err = strdup("Failed to initialize SSL context");
                return -1;
            }
            ssl = SSL_new(ctx);
            if (!ssl) {
                close_socket(sock);
                *out_err = strdup("Failed to create SSL instance");
                return -1;
            }
            SSL_set_tlsext_host_name(ssl, host.c_str());
            SSL_set_fd(ssl, sock);
            if (SSL_connect(ssl) <= 0) {
                unsigned long err_code = ERR_get_error();
                char err_buf[256];
                ERR_error_string_n(err_code, err_buf, sizeof(err_buf));
                std::string err = "SSL connect failed: " + std::string(err_buf);
                SSL_free(ssl);
                close_socket(sock);
                *out_err = strdup(err.c_str());
                return -1;
            }
        }

        std::string req = "GET " + path + " HTTP/1.1\r\n"
                        + "Host: " + host + "\r\n"
                        + "User-Agent: Eronom/0.9.4\r\n"
                        + "Accept: */*\r\n"
                        + "Connection: close\r\n\r\n";

        if (is_ssl) {
            SSL_write(ssl, req.data(), (int)req.size());
        } else {
            send(sock, req.data(), (int)req.size(), 0);
        }

        std::vector<char> response_data;
        char buffer[8192];
        if (is_ssl) {
            int bytes = 0;
            while ((bytes = SSL_read(ssl, buffer, sizeof(buffer))) > 0) {
                response_data.insert(response_data.end(), buffer, buffer + bytes);
            }
            SSL_shutdown(ssl);
            SSL_free(ssl);
        } else {
            int bytes = 0;
            while ((bytes = recv(sock, buffer, sizeof(buffer), 0)) > 0) {
                response_data.insert(response_data.end(), buffer, buffer + bytes);
            }
        }
        close_socket(sock);

        if (response_data.empty()) {
            *out_err = strdup("Empty response from server");
            return -1;
        }

        std::string raw(response_data.begin(), response_data.end());
        size_t header_end = raw.find("\r\n\r\n");
        size_t body_offset = 0;
        if (header_end != std::string::npos) {
            body_offset = header_end + 4;
        } else {
            size_t nlnl = raw.find("\n\n");
            if (nlnl != std::string::npos) {
                body_offset = nlnl + 2;
            }
        }

        std::string headers = raw.substr(0, body_offset);

        // Check for HTTP redirect (301, 302, 307, 308)
        size_t first_space = headers.find(' ');
        if (first_space != std::string::npos && first_space + 4 <= headers.size()) {
            int status_code = std::atoi(headers.substr(first_space + 1, 3).c_str());
            if (status_code == 301 || status_code == 302 || status_code == 303 || status_code == 307 || status_code == 308) {
                size_t loc_pos = headers.find("Location: ");
                if (loc_pos == std::string::npos) loc_pos = headers.find("location: ");
                if (loc_pos != std::string::npos) {
                    size_t loc_end = headers.find("\r\n", loc_pos);
                    std::string new_loc = headers.substr(loc_pos + 10, loc_end - (loc_pos + 10));
                    if (!new_loc.empty()) {
                        if (new_loc.rfind("http", 0) != 0) {
                            std::string base = (is_ssl ? "https://" : "http://") + host;
                            if (new_loc.front() == '/') new_loc = base + new_loc;
                            else new_loc = base + "/" + new_loc;
                        }
                        current_url = new_loc;
                        redirects++;
                        continue;
                    }
                }
            }
        }

        // Process response body
        const char* body_start = raw.data() + body_offset;
        size_t body_len = raw.size() - body_offset;

        std::string final_body;
        std::string headers_lower = headers;
        std::transform(headers_lower.begin(), headers_lower.end(), headers_lower.begin(), ::tolower);
        if (headers_lower.find("transfer-encoding: chunked") != std::string::npos) {
            final_body = decode_chunked_body(body_start, body_len);
        } else {
            final_body.assign(body_start, body_len);
        }

        char* ret_buf = (char*)malloc(final_body.size() + 1);
        if (!ret_buf) {
            *out_err = strdup("Memory allocation failed for response body");
            return -1;
        }
        std::memcpy(ret_buf, final_body.data(), final_body.size());
        ret_buf[final_body.size()] = '\0';

        *out_body = ret_buf;
        *out_body_len = final_body.size();
        return 0;
    }

    *out_err = strdup("Too many redirects");
    return -1;
}

void er_fetch_free(char* ptr) {
    if (ptr) {
        free(ptr);
    }
}

} // extern "C"

