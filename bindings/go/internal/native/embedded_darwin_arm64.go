package native

import "embed"

//go:embed embedded/aarch64-apple-darwin/*
var embeddedLibraries embed.FS

var embeddedLibraryNames = []string{"embedded/aarch64-apple-darwin/libtui_test_go.dylib"}
