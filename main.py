import sys
import logging
from contextlib import asynccontextmanager
from fastapi import FastAPI, Request
from fastapi.responses import PlainTextResponse
from todo.todo import router as todo_router

# Configure logging to match Go's default log format (YYYY/MM/DD HH:MM:SS) on stderr
logging.basicConfig(
    format='%(asctime)s %(message)s',
    datefmt='%Y/%m/%d %H:%M:%S',
    level=logging.INFO,
    stream=sys.stderr
)

@asynccontextmanager
async def lifespan(app: FastAPI):
    # Logs when the application starts up
    logging.info("Starting Todo API server on http://localhost:8080")
    yield
    # Logs when the application shuts down
    logging.info("Stopping Todo API server...")

# Disable automatic interactive docs to match Go API simplicity
app = FastAPI(lifespan=lifespan, openapi_url=None, docs_url=None, redoc_url=None)

@app.middleware("http")
async def log_requests(request: Request, call_next):
    # Log incoming request like Go middleware: RemoteAddr Method Path
    client_host = request.client.host if request.client else "unknown"
    client_port = request.client.port if request.client else 0
    logging.info(f"{client_host}:{client_port} {request.method} {request.url.path}")
    
    response = await call_next(request)
    return response

app.include_router(todo_router)

@app.get("/")
async def root():
    return PlainTextResponse("Welcome to the Python Todo API! Use GET/POST /todos, or PUT/DELETE /todos/{id}\n")

# Custom 404 page handler to match Go's default http.NotFound response
@app.exception_handler(404)
async def custom_404_handler(request: Request, exc):
    return PlainTextResponse("404 page not found\n", status_code=404)

if __name__ == "__main__":
    import uvicorn
    # Suppress uvicorn startup logs (level "warning") and access logs (since we do custom logging)
    uvicorn.run("main:app", host="0.0.0.0", port=8080, log_level="warning", access_log=False)
