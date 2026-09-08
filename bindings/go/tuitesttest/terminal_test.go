package tuitesttest_test

import (
	"bufio"
	"fmt"
	"os"
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

func TestTerminalSnapshot(t *testing.T) {
	actual := tuitesttest.TerminalSnapshot("one  \r\n two\t\n\n")
	if actual != "one\n two" {
		t.Fatalf("snapshot=%q", actual)
	}
}
