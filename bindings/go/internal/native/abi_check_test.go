//go:build tuitest_abi_check && cgo

package native

import (
	"reflect"
	"strings"
	"testing"
)

func TestNativeLayoutMatchesCompiler(t *testing.T) {
	for _, pair := range nativeLayoutPairs() {
		t.Run(pair.binding.Name(), func(t *testing.T) {
			if pair.binding.Size() != pair.compiler.Size() {
				t.Errorf("size: Go=%d, C=%d", pair.binding.Size(), pair.compiler.Size())
			}
			if pair.binding.Align() != pair.compiler.Align() {
				t.Errorf("alignment: Go=%d, C=%d", pair.binding.Align(), pair.compiler.Align())
			}
			compareNativeFields(t, pair)
		})
	}
}

func compareNativeFields(t *testing.T, pair nativeLayoutPair) {
	t.Helper()
	fields := nativeFields(pair.compiler)
	for name, field := range nativeFields(pair.binding) {
		native, found := fields[name]
		if !found {
			t.Errorf("field %s has no C counterpart", field.Name)
			continue
		}
		if field.Offset != native.Offset || field.Type.Size() != native.Type.Size() {
			t.Errorf("field %s: Go offset/size=%d/%d, C=%d/%d", field.Name, field.Offset, field.Type.Size(), native.Offset, native.Type.Size())
		}
		delete(fields, name)
	}
	for name := range fields {
		t.Errorf("C field %s has no Go counterpart", name)
	}
}

func nativeFields(layout reflect.Type) map[string]reflect.StructField {
	fields := make(map[string]reflect.StructField)
	for index := range layout.NumField() {
		field := layout.Field(index)
		if field.Name != "_" {
			fields[normalizeABIField(field.Name)] = field
		}
	}
	return fields
}

func normalizeABIField(name string) string {
	return strings.ToLower(strings.ReplaceAll(name, "_", ""))
}
