package strength

import "testing"

func TestString(t *testing.T) {
	if Inhibit.String() != "Inhibit" {
		t.Fatal(Inhibit.String())
	}
}
