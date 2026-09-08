package native

import (
	"os"
	"path/filepath"
	"sync"
	"testing"
)

func TestNativeCacheRepairsCorruptionAndSeparatesVersions(t *testing.T) {
	cache := t.TempDir()
	first := []byte("first engine version")
	firstPath, err := cacheNativeLibrary(cache, "engine.dll", first)
	if err != nil {
		t.Fatal(err)
	}
	if writeError := os.WriteFile(firstPath, []byte("corrupt"), 0o600); writeError != nil {
		t.Fatal(writeError)
	}
	repairedPath, err := cacheNativeLibrary(cache, "engine.dll", first)
	if err != nil {
		t.Fatal(err)
	}
	assertCachedContents(t, repairedPath, first)
	if repairedPath != firstPath {
		t.Fatal("cache did not restore the embedded engine at its original path")
	}
	secondPath, err := cacheNativeLibrary(cache, "engine.dll", []byte("second engine version"))
	if err != nil {
		t.Fatal(err)
	}
	if secondPath == firstPath {
		t.Fatal("different engine versions share a path")
	}
}

func TestNativeCacheSupportsConcurrentExtraction(t *testing.T) {
	cache := t.TempDir()
	contents := []byte("shared engine")
	var workers sync.WaitGroup
	for range 12 {
		workers.Go(func() {
			library, err := cacheNativeLibrary(cache, "engine.dll", contents)
			if err != nil {
				t.Error(err)
				return
			}
			assertCachedContents(t, library, contents)
		})
	}
	workers.Wait()
	library, err := cacheNativeLibrary(cache, "engine.dll", contents)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(filepath.Dir(library))
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Name() != "engine.dll" {
		t.Fatalf("unexpected files after concurrent extraction: %v", entries)
	}
}

func assertCachedContents(t *testing.T, library string, expected []byte) {
	t.Helper()
	cache, err := os.OpenRoot(filepath.Dir(library))
	if err != nil {
		t.Error(err)
		return
	}
	defer func() {
		if closeError := cache.Close(); closeError != nil {
			t.Error(closeError)
		}
	}()
	contents, err := cache.ReadFile(filepath.Base(library))
	if err != nil || string(contents) != string(expected) {
		t.Errorf("incomplete cached library: %q, %v", contents, err)
	}
}
