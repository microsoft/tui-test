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
	path := os.Getenv("TUI_TEST_GO_NATIVE_LIBRARY")
	if path == "" {
		return errors.New("TUI_TEST_GO_NATIVE_LIBRARY must name the native engine library")
	}
	table, err := loadNativeFunctions(path)
	if err != nil {
		return fmt.Errorf("load native engine from TUI_TEST_GO_NATIVE_LIBRARY: %w", err)
	}
	nativeFunctions = table
	return nil
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
	if version := table.AbiVersion(); version != 1 {
		return nativeFunctionTable{}, errors.Join(fmt.Errorf("native ABI version %d is incompatible with required version 1", version), library.close())
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
