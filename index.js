// server.js

const todos = [
    { id: 1, text: "Learn Eronom", done: false }
]

Bun.serve({
    port: 3000,

    async fetch(req) {
        const url = new URL(req.url)

        // GET /todos
        if (url.pathname === "/todos" && req.method === "GET") {
            return Response.json(todos)
        }

        // POST /todos
        if (url.pathname === "/todos" && req.method === "POST") {
            const body = await req.json()

            const newTodo = {
                id: todos.length + 1,
                text: body.text,
                done: false
            }

            todos.push(newTodo)

            return Response.json(newTodo, {
                status: 201
            })
        }

        // PUT /todos/:id
        if (url.pathname.startsWith("/todos/") && req.method === "PUT") {
            const id = Number(url.pathname.split("/")[2])
            const body = await req.json()

            const todo = todos.find(t => t.id === id)

            if (!todo) {
                return new Response("Todo not found", {
                    status: 404
                })
            }

            todo.done = body.done

            return Response.json(todo)
        }

        // DELETE /todos/:id
        if (url.pathname.startsWith("/todos/") && req.method === "DELETE") {
            const id = Number(url.pathname.split("/")[2])

            const index = todos.findIndex(t => t.id === id)

            if (index === -1) {
                return new Response("Todo not found", {
                    status: 404
                })
            }

            todos.splice(index, 1)

            return Response.json({
                message: "Deleted"
            })
        }

        return new Response("Not Found", {
            status: 404
        })
    }
})

console.log("Server running on http://localhost:3000")