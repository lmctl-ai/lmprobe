package repository

import (
	"errors"
	"fmt"
)

type User struct {
	ID   int
	Name string
}

type Repository interface {
	Find(id int) (*User, error)
	Save(u *User) error
}

type MemoryRepository struct {
	users map[int]*User
}

func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{users: make(map[int]*User)}
}

func (r *MemoryRepository) Find(id int) (*User, error) {
	if u, ok := r.users[id]; ok {
		return u, nil
	}
	return nil, errors.New("not found")
}

func (r *MemoryRepository) Save(u *User) error {
	r.users[u.ID] = u
	fmt.Println("saved", u.Name)
	return nil
}
