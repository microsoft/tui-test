package native

import (
	"fmt"
	"syscall"
)

type nativeLibraryHandle struct{ library *syscall.DLL }

func openNativeLibrary(path string) (*nativeLibraryHandle, error) {
	library, err := syscall.LoadDLL(path)
	if err != nil {
		return nil, fmt.Errorf("load native library %q: %w", path, err)
	}
	return &nativeLibraryHandle{library: library}, nil
}
func (handle *nativeLibraryHandle) symbol(name string) (uintptr, error) {
	procedure, err := handle.library.FindProc(name)
	if err != nil {
		return 0, fmt.Errorf("find native symbol %q: %w", name, err)
	}
	return procedure.Addr(), nil
}
func (handle *nativeLibraryHandle) close() error {
	if err := handle.library.Release(); err != nil {
		return fmt.Errorf("release native library: %w", err)
	}
	return nil
}
