package alpha

import (
	"testing"
	"example.com/coverage-selection/shared"
)

func TestAlpha(t *testing.T) {
	if shared.Alpha() != 3 { t.Fatal("wrong alpha result") }
}
