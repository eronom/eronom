from fastapi import FastAPI
import httpx

app = FastAPI()

@app.get("/todo")
async def get_todo():
    url = "https://jsonplaceholder.typicode.com/todos"
    async with httpx.AsyncClient() as client:
        response = await client.get(url)
    
    # Map the JSONPlaceholder API fields (title, completed)
    # to the fields expected by index.erm (text, done).
    # We display the first 10 items.
    data = response.json()
    if isinstance(data, list):
        return [
            {"text": item.get("title", ""), "done": item.get("completed", False)}
            for item in data[:10]
        ]
    elif isinstance(data, dict):
        return [{"text": data.get("title", ""), "done": data.get("completed", False)}]
    return []