from fastapi import APIRouter, Request, status
from fastapi.responses import JSONResponse
import json

router = APIRouter()

# Shared in-memory list of todos
todos = [
    {"id": 1, "text": "Learn Eronom", "done": False},
    {"id": 2, "text": "Add todo API in pyzzz", "done": True}
]
next_id = 3

@router.get("/todos")
async def get_todos():
    """Retrieve all todos."""
    return todos

@router.post("/todos")
async def create_todo(request: Request):
    """Create a new todo."""
    try:
        body = await request.body()
        input_data = json.loads(body.decode('utf-8'))
    except Exception:
        return JSONResponse(
            status_code=status.HTTP_400_BAD_REQUEST,
            content={"error": "Invalid input"}
        )

    text = input_data.get("text")
    if not isinstance(text, str) or text == "":
        return JSONResponse(
            status_code=status.HTTP_400_BAD_REQUEST,
            content={"error": "Text is required"}
        )

    global next_id
    new_todo = {
        "id": next_id,
        "text": text,
        "done": False
    }
    next_id += 1
    todos.append(new_todo)
    return JSONResponse(status_code=status.HTTP_201_CREATED, content=new_todo)

@router.put("/todos/{id}")
async def update_todo(id: str, request: Request):
    """Update an existing todo by ID."""
    try:
        todo_id = int(id)
    except ValueError:
        return JSONResponse(
            status_code=status.HTTP_400_BAD_REQUEST,
            content={"error": "Invalid ID"}
        )

    try:
        body = await request.body()
        input_data = json.loads(body.decode('utf-8'))
    except Exception:
        return JSONResponse(
            status_code=status.HTTP_400_BAD_REQUEST,
            content={"error": "Invalid input"}
        )

    for item in todos:
        if item["id"] == todo_id:
            if "text" in input_data and input_data["text"] is not None:
                item["text"] = str(input_data["text"])
            if "done" in input_data and input_data["done"] is not None:
                item["done"] = bool(input_data["done"])
            return item

    return JSONResponse(
        status_code=status.HTTP_404_NOT_FOUND,
        content={"error": "Todo not found"}
    )

@router.delete("/todos/{id}")
async def delete_todo(id: str):
    """Delete a todo by ID."""
    try:
        todo_id = int(id)
    except ValueError:
        return JSONResponse(
            status_code=status.HTTP_400_BAD_REQUEST,
            content={"error": "Invalid ID"}
        )

    for i, item in enumerate(todos):
        if item["id"] == todo_id:
            todos.pop(i)
            return {"status": "deleted"}

    return JSONResponse(
        status_code=status.HTTP_404_NOT_FOUND,
        content={"error": "Todo not found"}
    )
