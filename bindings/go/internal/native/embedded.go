package native

import (
	"crypto/rand"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
)

func embeddedNativeLibraries() ([]string, error) {
	if len(embeddedLibraryNames) == 0 {
		return nil, fmt.Errorf("bundled native engine does not support %s/%s", runtime.GOOS, runtime.GOARCH)
	}
	cacheDirectory, err := os.UserCacheDir()
	if err != nil {
		return nil, fmt.Errorf("locate native engine cache: %w", err)
	}
	libraries := make([]string, 0, len(embeddedLibraryNames))
	var failures []error
	for _, name := range embeddedLibraryNames {
		contents, readError := embeddedLibraries.ReadFile(name)
		if readError != nil {
			failures = append(failures, fmt.Errorf("read bundled engine %s: %w", name, readError))
			continue
		}
		library, extractError := cacheNativeLibrary(cacheDirectory, filepath.Base(name), contents)
		if extractError != nil {
			failures = append(failures, extractError)
			continue
		}
		libraries = append(libraries, library)
	}
	if len(libraries) == 0 {
		return nil, fmt.Errorf("no bundled native engine available for this platform (source checkouts require a native build; see CONTRIBUTING.md): %w", errors.Join(failures...))
	}
	return libraries, nil
}

// Each engine build has its own cache directory, so versions cannot overwrite
// libraries already loaded by another process. All file access stays in this root.
func cacheNativeLibrary(cacheDirectory, name string, contents []byte) (libraryPath string, resultError error) {
	digest := sha256.Sum256(contents)
	directory := filepath.Join(cacheDirectory, "tui-test", "native", fmt.Sprintf("%x", digest))
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return "", fmt.Errorf("create native engine cache: %w", err)
	}
	cache, err := os.OpenRoot(directory)
	if err != nil {
		return "", fmt.Errorf("open native engine cache: %w", err)
	}
	defer func() { resultError = errors.Join(resultError, cache.Close()) }()
	if err := ensureCachedLibrary(cache, name, contents); err != nil {
		return "", err
	}
	return filepath.Join(directory, name), nil
}

func ensureCachedLibrary(cache *os.Root, name string, contents []byte) (resultError error) {
	if cachedLibraryMatches(cache, name, contents) {
		return nil
	}
	temporaryName := ".engine-" + rand.Text()
	if err := cache.WriteFile(temporaryName, contents, 0o600); err != nil {
		return fmt.Errorf("write bundled native engine: %w", err)
	}
	defer func() {
		if err := cache.Remove(temporaryName); err != nil && !errors.Is(err, os.ErrNotExist) {
			resultError = errors.Join(resultError, fmt.Errorf("remove temporary native engine: %w", err))
		}
	}()
	if err := cache.Rename(temporaryName, name); err != nil {
		// Another process may have published this exact engine first. Windows
		// refuses to replace a DLL which that process has already loaded.
		if !cachedLibraryMatches(cache, name, contents) {
			return fmt.Errorf("publish bundled native engine: %w", err)
		}
	}
	return nil
}

func cachedLibraryMatches(cache *os.Root, name string, contents []byte) bool {
	cached, err := cache.ReadFile(name)
	return err == nil && sha256.Sum256(cached) == sha256.Sum256(contents)
}
