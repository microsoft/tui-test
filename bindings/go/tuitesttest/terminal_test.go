package tuitesttest_test

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"testing"
	"time"

	tuitest "github.com/microsoft/tui-test/bindings/go"
	"github.com/microsoft/tui-test/bindings/go/tuitesttest"
)

func TestHelperChild(t *testing.T) {
	if !slices.Contains(os.Args, "--helper-child") {
		return
	}
	fmt.Println("helper-ready")
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		if scanner.Text() == "quit" {
			os.Exit(0)
		}
	}
	os.Exit(0)
}

func helperOptions(t *testing.T) tuitesttest.Options {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return tuitesttest.Options{
		Client:  tuitest.ClientOptions{Recording: &tuitest.AutomaticRecording{Mode: tuitest.RecordingDisabled}},
		Spawn:   tuitest.SpawnOptions{WaitReady: tuitest.Ptr(false)},
		Program: executable, Args: []string{"-test.run=^TestHelperChild$", "--", "--helper-child"},
	}
}

func TestCleanupAndParallelIsolation(t *testing.T) {
	names := make(chan string, 2)
	t.Cleanup(func() {
		close(names)
		verifyClosedSessions(t, names)
	})
	for _, name := range []string{"one", "two"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			terminal := tuitesttest.New(t, helperOptions(t))
			names <- terminal.Session()
			if err := terminal.GetByText("helper-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(10 * time.Second)}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func verifyClosedSessions(t *testing.T, names <-chan string) {
	t.Helper()
	remaining, err := tuitest.Sessions()
	if err != nil {
		t.Fatal(err)
	}
	previous := ""
	for name := range names {
		if slices.Contains(remaining, name) {
			t.Errorf("session %s survived cleanup", name)
		}
		if previous == name {
			t.Errorf("parallel tests shared session %s", name)
		}
		previous = name
	}
}

func TestFailureArtifacts(t *testing.T) {
	if directory := os.Getenv("TUI_GO_HELPER_FAILURE_DIR"); directory != "" {
		runFailingTest(t, directory)
		return
	}
	directory := t.TempDir()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	command := exec.Command(executable, "-test.run=^TestFailureArtifacts$") //nolint:gosec // G204: Re-executes this test binary with fixed arguments to verify failure artifacts.
	command.Env = append(os.Environ(), "TUI_GO_HELPER_FAILURE_DIR="+directory)
	output, err := command.CombinedOutput()
	var exitError *exec.ExitError
	if !errors.As(err, &exitError) || exitError.ExitCode() != 1 {
		t.Fatalf("child error=%v output=%s", err, output)
	}
	matches, err := filepath.Glob(filepath.Join(directory, "*.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 1 {
		t.Fatalf("failure artifacts=%v output=%s", matches, output)
	}
}

func runFailingTest(t *testing.T, directory string) {
	t.Helper()
	options := helperOptions(t)
	options.Client.Artifacts = &tuitest.ArtifactOptions{Dir: directory, OnFailure: tuitest.ArtifactText}
	terminal := tuitesttest.New(t, options)
	if err := terminal.GetByText("helper-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(10 * time.Second)}); err != nil {
		t.Fatal(err)
	}
	t.Error("intentional failure to exercise registered cleanup")
}

func TestTerminalSnapshot(t *testing.T) {
	actual := tuitesttest.TerminalSnapshot("one  \r\n two\t\n\n")
	if actual != "one\n two" {
		t.Fatalf("snapshot=%q", actual)
	}
}
