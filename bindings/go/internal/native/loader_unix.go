//go:build linux || darwin || freebsd || netbsd

package native

import (
	"fmt"

	"github.com/ebitengine/purego"
)

type nativeLibraryHandle struct{ library uintptr }

func openNativeLibrary(path string) (*nativeLibraryHandle, error) {
	library, err := purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, fmt.Errorf("load native library %q: %w", path, err)
	}
	return &nativeLibraryHandle{library: library}, nil
}
func (handle *nativeLibraryHandle) symbol(name string) (uintptr, error) {
	address, err := purego.Dlsym(handle.library, name)
	if err != nil {
		return 0, fmt.Errorf("find native symbol %q: %w", name, err)
	}
	return address, nil
}
func (handle *nativeLibraryHandle) close() error {
	if err := purego.Dlclose(handle.library); err != nil {
		return fmt.Errorf("release native library: %w", err)
	}
	return nil
}
