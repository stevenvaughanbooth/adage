package aspect

//go:generate stringer -type=Aspect

// Aspect is a signal's strength.
type Aspect int

const (
	Green Aspect = iota
	Yellow
	Red
)
