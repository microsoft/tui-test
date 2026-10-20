package native

import (
	"strings"
	"testing"
)

func TestNativeLibraryPathRequiresBundleOrOverride(t *testing.T) {
	t.Setenv("TUI_TEST_NATIVE_LIBRARY", "")

	_, err := nativeLibraryPath(nil)
	if err == nil || !strings.Contains(err.Error(), "no bundled native engine") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestInitializeNativeEngineReportsConfiguredLibraryFailure(t *testing.T) {
	t.Setenv("TUI_TEST_NATIVE_LIBRARY", t.TempDir())

	err := initializeNativeEngine()
	if err == nil || !strings.Contains(err.Error(), "load native engine from") {
		t.Fatalf("unexpected error: %v", err)
	}
}
