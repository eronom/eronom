const http = require('http');

const todos = [
  { id: 1, text: "Learn Eronom - dynamic reload worked!ss for it", done: false }
];

const server = http.createServer((req, res) => {
  if (req.url === '/todos' && req.method === 'GET') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(todos));
  } else {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Not Found' }));
  }
});

server.listen(3001, () => {
  console.log('Node.js server listening on port 3001');
});
