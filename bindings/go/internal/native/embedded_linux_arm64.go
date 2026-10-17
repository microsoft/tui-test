package native

import "embed"

//go:embed embedded/aarch64-unknown-linux-gnu/*
//go:embed embedded/aarch64-unknown-linux-musl/*
var embeddedLibraries embed.FS

var embeddedLibraryNames = []string{"embedded/aarch64-unknown-linux-gnu/libtui_test_go.so", "embedded/aarch64-unknown-linux-musl/libtui_test_go.so"}
