// Package tuitesttest integrates terminal sessions with Go test cleanup.
package tuitesttest

import (
	"strings"
	"testing"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

// Options are per-test defaults; there is no process-wide mutable configuration.
type Options struct {
	Client  tuitest.ClientOptions
	Spawn   tuitest.SpawnOptions
	Shell   tuitest.Shell
	Program string
	Args    []string
	Prefix  string
}

// New opens a unique terminal and registers cleanup even if spawning fails.
// Test failures are reported through Close so the engine can retain configured traces.
func New(tb testing.TB, options Options) *tuitest.Client {
	tb.Helper()
	prefix := options.Prefix
	if prefix == "" {
		prefix = tb.Name()
	}
	terminal, err := tuitest.Ephemeral(prefix, options.Client)
	if err != nil {
		tb.Fatalf("create terminal: %v", err)
		return nil
	}
	tb.Cleanup(func() {
		failed := tb.Failed()
		if closeErr := terminal.Close(tuitest.CloseOptions{Failed: tuitest.Ptr(failed)}); closeErr != nil {
			tb.Errorf("close terminal: %v", closeErr)
		}
	})
	if options.Program == "" {
		_, err = terminal.Open(tuitest.OpenOptions{SpawnOptions: options.Spawn, Shell: options.Shell})
	} else {
		_, err = terminal.Run(options.Program, options.Args, options.Spawn)
	}
	if err != nil {
		tb.Fatalf("spawn terminal: %v", err)
	}
	return terminal
}

// TerminalSnapshot removes trailing whitespace and blank lines for text comparisons.
func TerminalSnapshot(text string) string {
	lines := strings.Split(text, "\n")
	for index, line := range lines {
		lines[index] = strings.TrimRight(line, " \t\r")
	}
	for len(lines) > 0 && lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	return strings.Join(lines, "\n")
}
