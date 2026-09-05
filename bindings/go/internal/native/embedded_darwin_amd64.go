package native

import "embed"

//go:embed embedded/x86_64-apple-darwin/*
var embeddedLibraries embed.FS

var embeddedLibraryNames = []string{"embedded/x86_64-apple-darwin/libtui_test_go.dylib"}
