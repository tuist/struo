package narrow

import "testing"

func TestValue(t *testing.T) {
	if Value() != 7 { t.Fatal("wrong wrapped result") }
}
