package native

import "embed"

//go:embed embedded/x86_64-unknown-linux-gnu/*
//go:embed embedded/x86_64-unknown-linux-musl/*
var embeddedLibraries embed.FS

var embeddedLibraryNames = []string{"embedded/x86_64-unknown-linux-gnu/libtui_test_go.so", "embedded/x86_64-unknown-linux-musl/libtui_test_go.so"}
