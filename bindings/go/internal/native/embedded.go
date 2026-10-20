package native

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/rand"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
)

type bundledLibrary struct {
	name     string
	checksum string
	archive  []byte
}

// Release packaging adds go:embed files for the target OS and architecture.
// Source checkouts leave this map empty and use TUI_TEST_NATIVE_LIBRARY.
var bundledLibraries = map[string]bundledLibrary{}

func nativeLibraryPath(libraries map[string]bundledLibrary) (string, error) {
	if path := os.Getenv("TUI_TEST_NATIVE_LIBRARY"); path != "" {
		return path, nil
	}
	target, err := bundleTarget(runtime.GOOS, runtime.GOARCH, nativeLibc())
	if err != nil {
		return "", err
	}
	library, ok := libraries[target]
	if !ok {
		return "", fmt.Errorf("no bundled native engine for %s: install a released Go module or set TUI_TEST_NATIVE_LIBRARY to a library from the same source revision", target)
	}
	cache, err := os.UserCacheDir()
	if err != nil {
		return "", fmt.Errorf("locate native engine cache: %w", err)
	}
	return cacheLibrary(cache, library)
}

func bundleTarget(goos, goarch, libc string) (string, error) {
	arch := map[string]string{"amd64": "x86_64", "arm64": "aarch64"}[goarch]
	if arch != "" {
		switch goos {
		case "darwin":
			return arch + "-apple-darwin", nil
		case "linux":
			if libc == "gnu" || libc == "musl" {
				return arch + "-unknown-linux-" + libc, nil
			}
		case "windows":
			if goarch == "amd64" {
				return arch + "-pc-windows-msvc", nil
			}
		}
	}
	return "", fmt.Errorf("no bundled native engine supports %s/%s with libc %q", goos, goarch, libc)
}

func cacheLibrary(cache string, library bundledLibrary) (path string, resultErr error) {
	directory := filepath.Join(cache, "tui-test", "native", library.checksum)
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return "", fmt.Errorf("create native engine cache: %w", err)
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return "", fmt.Errorf("open native engine cache: %w", err)
	}
	defer func() { resultErr = errors.Join(resultErr, root.Close()) }()
	path = filepath.Join(directory, library.name)
	if err := verifyCachedLibrary(root, library); err == nil {
		return path, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", fmt.Errorf("check native engine cache %q: %w", path, err)
	}

	temporaryName := ".engine-" + rand.Text()
	file, err := root.OpenFile(temporaryName, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return "", fmt.Errorf("create temporary native engine: %w", err)
	}
	defer func() {
		if err := root.Remove(temporaryName); err != nil {
			resultErr = errors.Join(resultErr, fmt.Errorf("remove temporary native engine: %w", err))
		}
	}()
	unpackErr := unpackLibrary(file, library)
	if err := errors.Join(unpackErr, file.Close()); err != nil {
		return "", fmt.Errorf("extract bundled native engine: %w", err)
	}
	// Publish only a complete file. Never replace an inode that another process
	// may already have mapped as executable code.
	if err := root.Link(temporaryName, library.name); err != nil {
		if !errors.Is(err, os.ErrExist) {
			return "", fmt.Errorf("publish native engine cache: %w", err)
		}
		if err := verifyCachedLibrary(root, library); err != nil {
			return "", fmt.Errorf("check concurrent native engine extraction: %w", err)
		}
	}
	return path, nil
}

func verifyCachedLibrary(root *os.Root, library bundledLibrary) error {
	file, err := root.Open(library.name)
	if err != nil {
		return err
	}
	hash := sha256.New()
	_, readErr := io.Copy(hash, file)
	if err := errors.Join(readErr, file.Close()); err != nil {
		return err
	}
	if fmt.Sprintf("%x", hash.Sum(nil)) != library.checksum {
		return fmt.Errorf("checksum mismatch; remove this cached file and run again")
	}
	return nil
}

func unpackLibrary(destination io.Writer, library bundledLibrary) error {
	compressed, err := gzip.NewReader(bytes.NewReader(library.archive))
	if err != nil {
		return err
	}
	defer compressed.Close()
	archive := tar.NewReader(compressed)
	found := false
	for {
		header, err := archive.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return err
		}
		if header.Name != library.name {
			continue
		}
		if found || header.Typeflag != tar.TypeReg {
			return fmt.Errorf("invalid archive entry for %s", library.name)
		}
		hash := sha256.New()
		if _, err := io.Copy(io.MultiWriter(destination, hash), archive); err != nil {
			return err
		}
		if fmt.Sprintf("%x", hash.Sum(nil)) != library.checksum {
			return fmt.Errorf("bundled native engine checksum mismatch")
		}
		found = true
	}
	if !found {
		return fmt.Errorf("archive does not contain %s", library.name)
	}
	// Consume the gzip trailer too, so a truncated archive cannot enter the cache.
	_, err = io.Copy(io.Discard, compressed)
	return err
}
