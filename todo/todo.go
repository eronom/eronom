package todo

import (
	"encoding/json"
	"log"
	"net/http"
	"strconv"
	"sync"
)

type Todo struct {
	ID   int    `json:"id"`
	Text string `json:"text"`
	Done bool   `json:"done"`
}

var (
	todos = []Todo{
		{ID: 1, Text: "Learn Eronom", Done: false},
		{ID: 2, Text: "Add todo API in Gosssssvs gooooo", Done: true},
	}
	nextID = 3
	mu     sync.Mutex
)

// Helper to write JSON responses
func WriteJSON(w http.ResponseWriter, status int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(data); err != nil {
		log.Printf("Error encoding JSON: %v", err)
	}
}

func HandleGetTodos(w http.ResponseWriter, r *http.Request) {
	mu.Lock()
	defer mu.Unlock()
	WriteJSON(w, http.StatusOK, todos)
}

func HandleCreateTodo(w http.ResponseWriter, r *http.Request) {
	var input struct {
		Text string `json:"text"`
	}
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		WriteJSON(w, http.StatusBadRequest, map[string]string{"error": "Invalid input"})
		return
	}
	if input.Text == "" {
		WriteJSON(w, http.StatusBadRequest, map[string]string{"error": "Text is required"})
		return
	}

	mu.Lock()
	newTodo := Todo{
		ID:   nextID,
		Text: input.Text,
		Done: false,
	}
	nextID++
	todos = append(todos, newTodo)
	mu.Unlock()

	WriteJSON(w, http.StatusCreated, newTodo)
}

func HandleUpdateTodo(w http.ResponseWriter, r *http.Request) {
	idStr := r.PathValue("id")
	id, err := strconv.Atoi(idStr)
	if err != nil {
		WriteJSON(w, http.StatusBadRequest, map[string]string{"error": "Invalid ID"})
		return
	}

	var input struct {
		Text *string `json:"text"`
		Done *bool   `json:"done"`
	}
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		WriteJSON(w, http.StatusBadRequest, map[string]string{"error": "Invalid input"})
		return
	}

	mu.Lock()
	defer mu.Unlock()

	for i, todo := range todos {
		if todo.ID == id {
			if input.Text != nil {
				todos[i].Text = *input.Text
			}
			if input.Done != nil {
				todos[i].Done = *input.Done
			}
			WriteJSON(w, http.StatusOK, todos[i])
			return
		}
	}

	WriteJSON(w, http.StatusNotFound, map[string]string{"error": "Todo not found"})
}

func HandleDeleteTodo(w http.ResponseWriter, r *http.Request) {
	idStr := r.PathValue("id")
	id, err := strconv.Atoi(idStr)
	if err != nil {
		WriteJSON(w, http.StatusBadRequest, map[string]string{"error": "Invalid ID"})
		return
	}

	mu.Lock()
	defer mu.Unlock()

	for i, todo := range todos {
		if todo.ID == id {
			todos = append(todos[:i], todos[i+1:]...)
			WriteJSON(w, http.StatusOK, map[string]string{"status": "deleted"})
			return
		}
	}

	WriteJSON(w, http.StatusNotFound, map[string]string{"error": "Todo not found"})
}
