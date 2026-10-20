package native

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
)

func libraryFixture(t *testing.T, contents string) bundledLibrary {
	t.Helper()
	var buffer bytes.Buffer
	compressed := gzip.NewWriter(&buffer)
	archive := tar.NewWriter(compressed)
	name := "library.so"
	if err := archive.WriteHeader(&tar.Header{Name: name, Mode: 0o600, Size: int64(len(contents))}); err != nil {
		t.Fatal(err)
	}
	if _, err := archive.Write([]byte(contents)); err != nil {
		t.Fatal(err)
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	if err := compressed.Close(); err != nil {
		t.Fatal(err)
	}
	return bundledLibrary{name: name, checksum: fmt.Sprintf("%x", sha256.Sum256([]byte(contents))), archive: buffer.Bytes()}
}

func TestBundleTarget(t *testing.T) {
	for _, tt := range []struct{ os, arch, libc, target string }{
		{"linux", "amd64", "gnu", "x86_64-unknown-linux-gnu"},
		{"linux", "arm64", "gnu", "aarch64-unknown-linux-gnu"},
		{"linux", "amd64", "musl", "x86_64-unknown-linux-musl"},
		{"linux", "arm64", "musl", "aarch64-unknown-linux-musl"},
		{"darwin", "amd64", "", "x86_64-apple-darwin"},
		{"darwin", "arm64", "", "aarch64-apple-darwin"},
		{"windows", "amd64", "", "x86_64-pc-windows-msvc"},
		{"windows", "arm64", "", ""},
		{"linux", "386", "gnu", ""},
		{"linux", "amd64", "other", ""},
		{"freebsd", "amd64", "", ""},
	} {
		t.Run(tt.os+"/"+tt.arch+"/"+tt.libc, func(t *testing.T) {
			target, err := bundleTarget(tt.os, tt.arch, tt.libc)
			if target != tt.target || (err != nil) != (tt.target == "") {
				t.Fatalf("target = %q, error = %v", target, err)
			}
		})
	}
}

func TestOverrideBypassesBundleAndCache(t *testing.T) {
	t.Setenv("TUI_TEST_NATIVE_LIBRARY", "/explicit/library")
	path, err := nativeLibraryPath(nil)
	if err != nil || path != "/explicit/library" {
		t.Fatalf("path = %q, error = %v", path, err)
	}
}

func TestNativeLibraryPathSelectsProcessTarget(t *testing.T) {
	t.Setenv("TUI_TEST_NATIVE_LIBRARY", "")
	cache := t.TempDir()
	t.Setenv("HOME", cache)
	t.Setenv("XDG_CACHE_HOME", cache)
	t.Setenv("LocalAppData", cache)
	target, err := bundleTarget(runtime.GOOS, runtime.GOARCH, nativeLibc())
	if err != nil {
		t.Fatal(err)
	}
	library := libraryFixture(t, "selected library")
	path, err := nativeLibraryPath(map[string]bundledLibrary{target: library})
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(path)
	if err != nil || string(data) != "selected library" {
		t.Fatalf("data = %q, error = %v", data, err)
	}
}

func TestCacheLibraryReusesFileAndSeparatesVersions(t *testing.T) {
	cache := t.TempDir()
	library := libraryFixture(t, "first release")
	path, err := cacheLibrary(cache, library)
	if err != nil {
		t.Fatal(err)
	}
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	library.archive = nil
	again, err := cacheLibrary(cache, library)
	if err != nil || again != path {
		t.Fatalf("path = %q, error = %v", again, err)
	}
	after, err := os.Stat(path)
	if err != nil || !os.SameFile(before, after) {
		t.Fatalf("cached file was replaced: %v", err)
	}
	next, err := cacheLibrary(cache, libraryFixture(t, "second release"))
	if err != nil || next == path {
		t.Fatalf("new release path = %q, error = %v", next, err)
	}
}

func TestCacheLibraryRejectsCorruptFile(t *testing.T) {
	cache := t.TempDir()
	library := libraryFixture(t, "correct library")
	path, err := cacheLibrary(cache, library)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("corrupt"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := cacheLibrary(cache, library); err == nil || !strings.Contains(err.Error(), "checksum mismatch") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestCacheLibraryRejectsInvalidArchive(t *testing.T) {
	for _, kind := range []string{"missing", "checksum", "gzip", "truncated"} {
		t.Run(kind, func(t *testing.T) {
			cache := t.TempDir()
			library := libraryFixture(t, "correct library")
			switch kind {
			case "missing":
				library.name = "missing.so"
			case "checksum":
				library.checksum = strings.Repeat("0", 64)
			case "gzip":
				library.archive = []byte("not a gzip file")
			case "truncated":
				library.archive = library.archive[:len(library.archive)-4]
			}
			if _, err := cacheLibrary(cache, library); err == nil {
				t.Fatal("expected an extraction error")
			}
			entries, err := os.ReadDir(filepath.Join(cache, "tui-test", "native", library.checksum))
			if err != nil || len(entries) != 0 {
				t.Fatalf("failed extraction left files: %v, %v", entries, err)
			}
		})
	}
}

func TestCacheLibraryRejectsUnusableCache(t *testing.T) {
	cache := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(cache, []byte("not a directory"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := cacheLibrary(cache, libraryFixture(t, "library")); err == nil {
		t.Fatal("expected a cache error")
	}
}

func TestCacheLibraryConcurrentProcesses(t *testing.T) {
	cache := t.TempDir()
	var group sync.WaitGroup
	for range 8 {
		group.Go(func() {
			command := exec.Command(os.Args[0], "-test.run=^TestCacheLibraryChild$")
			command.Env = append(os.Environ(), "TUI_TEST_EXTRACTION_TEST_CACHE="+cache)
			if output, err := command.CombinedOutput(); err != nil {
				t.Errorf("child failed: %v\n%s", err, output)
			}
		})
	}
	group.Wait()
	library := libraryFixture(t, strings.Repeat("native library", 10000))
	path, err := cacheLibrary(cache, library)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(filepath.Dir(path))
	if err != nil || len(entries) != 1 || entries[0].Name() != library.name {
		t.Fatalf("unexpected cache entries: %v, %v", entries, err)
	}
}

func TestCacheLibraryChild(t *testing.T) {
	cache := os.Getenv("TUI_TEST_EXTRACTION_TEST_CACHE")
	if cache == "" {
		return
	}
	if _, err := cacheLibrary(cache, libraryFixture(t, strings.Repeat("native library", 10000))); err != nil {
		t.Fatal(err)
	}
}
