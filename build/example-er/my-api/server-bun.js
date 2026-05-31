const todos = [
  { id: 1, text: "Learn Eronom - dynamic reload worked!ss for it", done: false }
];

// Middleware to attach a value to the context
const middlewares = [
  (c) => {
    c.user = "Alice";
  }
];

Bun.serve({
  port: 3002,
  fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === '/todos' && req.method === 'GET') {
      const c = {
        req,
        json: (data) => Response.json(data)
      };

      // Run middlewares
      for (const mw of middlewares) {
        mw(c);
      }

      console.log("User attached by middleware: " + c.user);
      return c.json(todos);
    } else {
      return Response.json({ error: 'Not Found' }, { status: 404 });
    }
  }
});

console.log('Bun server listening on port 3002');

