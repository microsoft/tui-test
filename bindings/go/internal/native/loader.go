package native

import (
	"errors"
	"fmt"
	"os"
	"sync"

	"github.com/ebitengine/purego"
)

type nativeRegistration struct {
	symbol string
	target any
}

// Registered functions and the loaded library stay live for the process lifetime.
var nativeFunctions nativeFunctionTable
var loadNativeEngine = sync.OnceValue(initializeNativeEngine)

func initializeNativeEngine() error {
	paths, source, err := nativeLibraryPaths()
	if err != nil {
		return fmt.Errorf("prepare native engine: %w", err)
	}
	failures := make([]error, 0, len(paths))
	for _, path := range paths {
		table, loadErr := loadNativeFunctions(path)
		if loadErr == nil {
			nativeFunctions = table
			return nil
		}
		failures = append(failures, loadErr)
	}
	if len(failures) == 0 {
		return errors.New("no embedded native engine is available for this platform")
	}
	return fmt.Errorf("load %s: %w", source, errors.Join(failures...))
}

func nativeLibraryPaths() ([]string, string, error) {
	if path := os.Getenv("TUI_TEST_GO_NATIVE_LIBRARY"); path != "" {
		return []string{path}, "native engine configured by TUI_TEST_GO_NATIVE_LIBRARY", nil
	}
	paths, err := embeddedNativeLibraries()
	return paths, "embedded native engine", err
}

func loadNativeFunctions(path string) (nativeFunctionTable, error) {
	table := nativeFunctionTable{}
	library, err := openNativeLibrary(path)
	if err != nil {
		return table, err
	}
	for _, registration := range nativeRegistrations(&table) {
		if registerErr := registerNativeFunction(library, registration); registerErr != nil {
			return nativeFunctionTable{}, errors.Join(registerErr, library.close())
		}
	}
	if version := table.AbiVersion(); version != 4 {
		return nativeFunctionTable{}, errors.Join(fmt.Errorf("native ABI version %d is incompatible with required version 4", version), library.close())
	}
	return table, nil
}

func registerNativeFunction(library *nativeLibraryHandle, registration nativeRegistration) (err error) {
	address, err := library.symbol(registration.symbol)
	if err != nil {
		return err
	}
	defer func() {
		if failure := recover(); failure != nil {
			err = fmt.Errorf("register native function %s: %v", registration.symbol, failure)
		}
	}()
	purego.RegisterFunc(registration.target, address)
	return nil
}
