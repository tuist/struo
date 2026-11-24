package beta

import (
	"testing"
	"example.com/coverage-selection/shared"
)

func TestBeta(t *testing.T) {
	if shared.Beta() != 7 { t.Fatal("wrong beta result") }
}
