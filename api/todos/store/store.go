package store

import (
	"fmt"
	"sync"
	"time"
)

// Todo state for the API
type Todo struct {
	ID        int       `json:"id"`
	Text      string    `json:"text"`
	Done      bool      `json:"done"`
	CreatedAt time.Time `json:"createdAt"`
}

var (
	todos  = []Todo{}
	todoID = 0
	todoMu sync.Mutex
)

// GetAll returns all todos
func GetAll() []Todo {
	todoMu.Lock()
	defer todoMu.Unlock()
	return todos
}

// Add adds a new todo
func Add(text string) Todo {
	todoMu.Lock()
	defer todoMu.Unlock()
	todoID++
	todo := Todo{
		ID:        todoID,
		Text:      text,
		Done:      false,
		CreatedAt: time.Now(),
	}
	todos = append(todos, todo)
	return todo
}

// GetByID returns a todo by ID
func GetByID(id string) (*Todo, bool) {
	todoMu.Lock()
	defer todoMu.Unlock()
	for _, t := range todos {
		if fmt.Sprintf("%d", t.ID) == id {
			return &t, true
		}
	}
	return nil, false
}

// Delete removes a todo by ID
func Delete(id int) bool {
	todoMu.Lock()
	defer todoMu.Unlock()
	for i, t := range todos {
		if t.ID == id {
			todos = append(todos[:i], todos[i+1:]...)
			return true
		}
	}
	return false
}

// Update updates a todo's done status
func Update(id int, done bool) (*Todo, bool) {
	todoMu.Lock()
	defer todoMu.Unlock()
	for i := range todos {
		if todos[i].ID == id {
			todos[i].Done = done
			return &todos[i], true
		}
	}
	return nil, false
}
