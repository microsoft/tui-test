// Package tuitesttest integrates terminal sessions with Go test cleanup.
package tuitesttest

import (
	"fmt"
	"os"
	"path/filepath"
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
// If Client.Artifacts is configured, failed tests save artifacts before closing.
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
		if tb.Failed() {
			captureFailure(tb, terminal, options.Client.Artifacts)
		}
		if closeErr := terminal.Close(); closeErr != nil {
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

func captureFailure(tb testing.TB, terminal *tuitest.Client, options *tuitest.ArtifactOptions) {
	tb.Helper()
	if options == nil || options.OnFailure == tuitest.ArtifactNone {
		return
	}
	if err := os.MkdirAll(options.Dir, 0755); err != nil {
		tb.Logf("terminal artifact: %v", err)
		return
	}
	filename := filepath.Join(options.Dir, terminal.Session())
	path, err := saveArtifact(terminal, filename, options.OnFailure)
	if err != nil {
		tb.Logf("terminal artifact: %v", err)
	} else {
		tb.Logf("terminal artifact: %s", path)
	}
}

func saveArtifact(terminal *tuitest.Client, filename string, mode tuitest.ArtifactMode) (string, error) {
	if mode != tuitest.ArtifactText {
		path, err := terminal.Screenshot(filename+".svg", tuitest.ScreenshotOptions{})
		if err != nil {
			return "", fmt.Errorf("save terminal screenshot: %w", err)
		}
		return path, nil
	}
	text, err := terminal.Text(tuitest.TextOptions{})
	if err != nil {
		return "", fmt.Errorf("read terminal text: %w", err)
	}
	path := filename + ".txt"
	if err = os.WriteFile(path, []byte(text), 0644); err != nil {
		return "", fmt.Errorf("save terminal text: %w", err)
	}
	return path, nil
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
