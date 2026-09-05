//go:build !((windows && amd64) || ((darwin || linux) && (amd64 || arm64)))

package native

import "embed"

var embeddedLibraries embed.FS
var embeddedLibraryNames []string
