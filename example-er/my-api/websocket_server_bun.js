Bun.serve({
  port: 3002,
  fetch(req, server) {
    if (server.upgrade(req)) {
      return;
    }
    return new Response("Upgrade failed", { status: 500 });
  },
  websocket: {
    message(ws, message) {
      ws.send("Echo: " + message);
    },
    open(ws) {
    },
    close(ws, code, message) {
    }
  }
});

console.log("Bun WebSocket server listening on port 3002");
