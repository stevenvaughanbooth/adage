package strength

//go:generate stringer -type=Strength

// Strength is how strongly a reflex speaks.
type Strength int

const (
	Fire Strength = iota
	Prime
	Inhibit
)
