//go:build !linux

package native

func nativeLibc() string { return "" }
