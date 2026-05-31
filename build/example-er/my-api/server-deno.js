const todos = [
  { id: 1, text: "Learn Eronom - dynamic reload worked!ss for it", done: false }
];

Deno.serve({
  port: 3003,
  onListen({ port }) {
    console.log(`Deno server listening on port ${port}`);
  }
}, (req) => {
  const url = new URL(req.url);
  if (url.pathname === '/todos' && req.method === 'GET') {
    return Response.json(todos);
  } else {
    return Response.json({ error: 'Not Found' }, { status: 404 });
  }
});
