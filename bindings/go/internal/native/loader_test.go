package native

import (
	"strings"
	"testing"
)

func TestInitializeNativeEngineRequiresConfiguredLibrary(t *testing.T) {
	t.Setenv("TUI_TEST_GO_NATIVE_LIBRARY", "")

	err := initializeNativeEngine()
	if err == nil || !strings.Contains(err.Error(), "TUI_TEST_GO_NATIVE_LIBRARY must name") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestInitializeNativeEngineReportsConfiguredLibraryFailure(t *testing.T) {
	t.Setenv("TUI_TEST_GO_NATIVE_LIBRARY", t.TempDir())

	err := initializeNativeEngine()
	if err == nil || !strings.Contains(err.Error(), "load native engine from TUI_TEST_GO_NATIVE_LIBRARY") {
		t.Fatalf("unexpected error: %v", err)
	}
}
