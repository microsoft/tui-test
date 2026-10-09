package native

import (
	"path/filepath"
	"strings"
	"testing"
)

func TestNativeLibraryPathsPreferConfiguredLibrary(t *testing.T) {
	configuredPath := filepath.Join(t.TempDir(), "tui-test-native")
	t.Setenv("TUI_TEST_GO_NATIVE_LIBRARY", configuredPath)

	paths, source, err := nativeLibraryPaths()
	if err != nil {
		t.Fatal(err)
	}
	if len(paths) != 1 || paths[0] != configuredPath {
		t.Fatalf("unexpected native library paths: %v", paths)
	}
	if source != "native engine configured by TUI_TEST_GO_NATIVE_LIBRARY" {
		t.Fatalf("unexpected native library source: %s", source)
	}
}

func TestInitializeNativeEngineReportsConfiguredLibraryFailure(t *testing.T) {
	t.Setenv("TUI_TEST_GO_NATIVE_LIBRARY", filepath.Join(t.TempDir(), "missing-native-library"))

	err := initializeNativeEngine()
	if err == nil || !strings.Contains(err.Error(), "load native engine configured by TUI_TEST_GO_NATIVE_LIBRARY") {
		t.Fatalf("unexpected error: %v", err)
	}
}
