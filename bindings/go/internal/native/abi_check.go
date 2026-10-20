//go:build tuitest_abi_check && cgo

package native

/*
#cgo CFLAGS: -I${SRCDIR}/../../../../crates/tui-test-ffi/include
#include "tui_test.h"
*/
import "C"

import "reflect"

const nativeCompilerABIVersion = C.ABI_VERSION

type nativeLayoutPair struct{ binding, compiler reflect.Type }

// This opt-in check uses the shared header from a source checkout.
func nativeLayoutPairs() []nativeLayoutPair {
	return []nativeLayoutPair{
		{reflect.TypeFor[abiString](), reflect.TypeFor[C.TuiString]()},
		{reflect.TypeFor[abiOptionalI32](), reflect.TypeFor[C.TuiOptionalI32]()},
		{reflect.TypeFor[abiOptionalU64](), reflect.TypeFor[C.TuiOptionalU64]()},
		{reflect.TypeFor[abiOptionalBool](), reflect.TypeFor[C.TuiOptionalBool]()},
		{reflect.TypeFor[abiOptionalF64](), reflect.TypeFor[C.TuiOptionalF64]()},
		{reflect.TypeFor[abiPair](), reflect.TypeFor[C.TuiPair]()},
		{reflect.TypeFor[abiQuery](), reflect.TypeFor[C.TuiQuery]()},
		{reflect.TypeFor[abiOpenResult](), reflect.TypeFor[C.TuiOpenResult]()},
		{reflect.TypeFor[abiCursor](), reflect.TypeFor[C.TuiCursor]()},
		{reflect.TypeFor[abiMode](), reflect.TypeFor[C.TuiMode]()},
		{reflect.TypeFor[abiTerminalColors](), reflect.TypeFor[C.TuiTerminalColors]()},
		{reflect.TypeFor[abiTimeouts](), reflect.TypeFor[C.TuiTimeouts]()},
		{reflect.TypeFor[abiState](), reflect.TypeFor[C.TuiState]()},
		{reflect.TypeFor[abiSize](), reflect.TypeFor[C.TuiSize]()},
		{reflect.TypeFor[abiColor](), reflect.TypeFor[C.TuiColor]()},
		{reflect.TypeFor[abiCell](), reflect.TypeFor[C.TuiCell]()},
		{reflect.TypeFor[abiPosition](), reflect.TypeFor[C.TuiPosition]()},
		{reflect.TypeFor[abiSpan](), reflect.TypeFor[C.TuiSpan]()},
		{reflect.TypeFor[abiMatch](), reflect.TypeFor[C.TuiMatch]()},
		{reflect.TypeFor[abiBellEvent](), reflect.TypeFor[C.TuiBellEvent]()},
		{reflect.TypeFor[abiResult](), reflect.TypeFor[C.TuiResult]()},
		{reflect.TypeFor[abiOpenOptions](), reflect.TypeFor[C.TuiOpenOptions]()},
		{reflect.TypeFor[abiMouseOptions](), reflect.TypeFor[C.TuiMouseOptions]()},
		{reflect.TypeFor[abiWaitOptions](), reflect.TypeFor[C.TuiWaitOptions]()},
		{reflect.TypeFor[abiRecordingOptions](), reflect.TypeFor[C.TuiRecordingOptions]()},
		{reflect.TypeFor[abiScreenshotOptions](), reflect.TypeFor[C.TuiScreenshotOptions]()},
	}
}
