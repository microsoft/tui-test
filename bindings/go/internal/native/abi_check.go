//go:build tuitest_abi_check && cgo

package native

/*
#include "native.h"
*/
import "C"

import (
	"reflect"
	"slices"
)

type nativeLayoutPair struct{ binding, compiler reflect.Type }

// The C compiler supplies its actual native.h field offsets for this target.
func nativeLayoutPairs() []nativeLayoutPair {
	return slices.Concat(nativeOutputLayouts(), nativeInputLayouts())
}

func nativeOutputLayouts() []nativeLayoutPair {
	return []nativeLayoutPair{
		{binding: reflect.TypeFor[abiString](), compiler: reflect.TypeFor[C.TuiString]()},
		{binding: reflect.TypeFor[abiOptionalI32](), compiler: reflect.TypeFor[C.TuiOptionalI32]()},
		{binding: reflect.TypeFor[abiOptionalU64](), compiler: reflect.TypeFor[C.TuiOptionalU64]()},
		{binding: reflect.TypeFor[abiOpenResult](), compiler: reflect.TypeFor[C.TuiOpenResult]()},
		{binding: reflect.TypeFor[abiCursor](), compiler: reflect.TypeFor[C.TuiCursor]()},
		{binding: reflect.TypeFor[abiTimeouts](), compiler: reflect.TypeFor[C.TuiTimeouts]()},
		{binding: reflect.TypeFor[abiState](), compiler: reflect.TypeFor[C.TuiState]()},
		{binding: reflect.TypeFor[abiSize](), compiler: reflect.TypeFor[C.TuiSize]()},
		{binding: reflect.TypeFor[abiColor](), compiler: reflect.TypeFor[C.TuiColor]()},
		{binding: reflect.TypeFor[abiCell](), compiler: reflect.TypeFor[C.TuiCell]()},
		{binding: reflect.TypeFor[abiPosition](), compiler: reflect.TypeFor[C.TuiPosition]()},
		{binding: reflect.TypeFor[abiSpan](), compiler: reflect.TypeFor[C.TuiSpan]()},
		{binding: reflect.TypeFor[abiMatch](), compiler: reflect.TypeFor[C.TuiMatch]()},
		{binding: reflect.TypeFor[abiBellEvent](), compiler: reflect.TypeFor[C.TuiBellEvent]()},
		{binding: reflect.TypeFor[abiResult](), compiler: reflect.TypeFor[C.TuiResult]()},
	}
}

func nativeInputLayouts() []nativeLayoutPair {
	return []nativeLayoutPair{
		{binding: reflect.TypeFor[abiPair](), compiler: reflect.TypeFor[C.TuiPair]()},
		{binding: reflect.TypeFor[abiOptionalBool](), compiler: reflect.TypeFor[C.TuiOptionalBool]()},
		{binding: reflect.TypeFor[abiOpenOptions](), compiler: reflect.TypeFor[C.TuiOpenOptions]()},
		{binding: reflect.TypeFor[abiMouseOptions](), compiler: reflect.TypeFor[C.TuiMouseOptions]()},
		{binding: reflect.TypeFor[abiWaitOptions](), compiler: reflect.TypeFor[C.TuiWaitOptions]()},
		{binding: reflect.TypeFor[abiAnchor](), compiler: reflect.TypeFor[C.TuiAnchor]()},
		{binding: reflect.TypeFor[abiTextStyle](), compiler: reflect.TypeFor[C.TuiTextStyle]()},
		{binding: reflect.TypeFor[abiLocatorStage](), compiler: reflect.TypeFor[C.TuiLocatorStage]()},
		{binding: reflect.TypeFor[abiQuery](), compiler: reflect.TypeFor[C.TuiQuery]()},
		{binding: reflect.TypeFor[abiOptionalF64](), compiler: reflect.TypeFor[C.TuiOptionalF64]()},
		{binding: reflect.TypeFor[abiRecordingOptions](), compiler: reflect.TypeFor[C.TuiRecordingOptions]()},
	}
}
