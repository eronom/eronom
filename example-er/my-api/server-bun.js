const todos = [
  { id: 1, text: "Learn Eronom - dynamic reload worked!ss for it", done: false }
];

Bun.serve({
  port: 3002,
  fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === '/todos' && req.method === 'GET') {
      return Response.json(todos);
    } else {
      return Response.json({ error: 'Not Found' }, { status: 404 });
    }
  }
});

console.log('Bun server listening on port 3002');
