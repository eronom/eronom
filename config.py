import sys
import os
import asyncio
import importlib.util

# Add current directory to path
sys.path.insert(0, os.getcwd())

# Get the user's route file dynamically from environment variable
route_file = os.environ.get("ROUTE_FILE_PATH")
if not route_file:
    raise RuntimeError("ROUTE_FILE_PATH environment variable not set")

spec = importlib.util.spec_from_file_location("route_module", route_file)
module = importlib.util.module_from_spec(spec)
sys.modules["route_module"] = module
spec.loader.exec_module(module)

# Find the FastAPI app
app = getattr(module, "app", None)
if not app:
    from fastapi import FastAPI
    for attr in dir(module):
        val = getattr(module, attr)
        if isinstance(val, FastAPI):
            app = val
            break

if not app:
    raise RuntimeError(f"No FastAPI app found in {route_file}")

# ASGI-to-CGI Runner
async def run_asgi(app, method, path, query, body):
    scope = {
        "type": "http",
        "asgi": {"version": "3.0", "spec_version": "2.0"},
        "http_version": "1.1",
        "method": method,
        "path": path,
        "raw_path": path.encode("utf-8"),
        "query_string": query.encode("utf-8"),
        "headers": [],
    }
    async def receive():
        return {"type": "http.request", "body": body, "more_body": False}
    status_code = 200
    headers = []
    body_chunks = []
    async def send(msg):
        nonlocal status_code
        if msg["type"] == "http.response.start":
            status_code = msg["status"]
            for k, v in msg.get("headers", []):
                headers.append((k.decode("latin1"), v.decode("latin1")))
        elif msg["type"] == "http.response.body":
            body_chunks.append(msg.get("body", b""))
    await app(scope, receive, send)
    return status_code, headers, b"".join(body_chunks)

method = os.environ.get("REQUEST_METHOD", "GET").upper()
full_path = os.environ.get("REQUEST_PATH", "/")
if "?" in full_path:
    path, query = full_path.split("?", 1)
else:
    path, query = full_path, os.environ.get("QUERY_STRING", "")

body_str = os.environ.get("REQUEST_BODY", "")
body_bytes = body_str.encode("utf-8")

if path.startswith("/api"):
    path = path[4:]
    if not path.startswith("/"):
        path = "/" + path

status_code, headers, body_res = asyncio.run(run_asgi(app, method, path, query, body_bytes))

print(f"Status: {status_code}")
for k, v in headers:
    print(f"{k}: {v}")
print()
print(body_res.decode("utf-8"))
