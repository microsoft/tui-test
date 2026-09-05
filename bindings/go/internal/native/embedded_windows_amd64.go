package native

import "embed"

//go:embed embedded/x86_64-pc-windows-msvc/*
var embeddedLibraries embed.FS

var embeddedLibraryNames = []string{"embedded/x86_64-pc-windows-msvc/tui_test_go.dll"}
