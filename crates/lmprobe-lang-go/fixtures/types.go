package types

const MaxRetries = 3

var DefaultTimeout = 30

type Status = string

type Point struct {
	X, Y int
}

type Greeter interface {
	Greet(name string) string
}
