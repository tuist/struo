package greeting

import (
	"os"
	"strings"
	"testing"
)

func TestMessageMatchesGolden(t *testing.T) {
	want, err := os.ReadFile("testdata/world.golden")
	if err != nil {
		t.Fatal(err)
	}
	if got := Message("world"); got != strings.TrimSpace(string(want)) {
		t.Fatalf("Message(world) = %q, want %q", got, want)
	}
}
