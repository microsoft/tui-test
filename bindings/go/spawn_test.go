package tuitest_test

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func TestRetryTerminalProcess(t *testing.T) {
	if !slices.Contains(os.Args, "--tui-go-retry-child") {
		return
	}
	const marker = "first-launch"
	_, err := os.Stat(marker)
	if errors.Is(err, os.ErrNotExist) {
		requireNoError(t, os.WriteFile(marker, []byte("first launch"), 0600))
	} else {
		requireNoError(t, err)
		fmt.Print("\x1b]133;B\x07retry-ready\r\n")
	}
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		fmt.Printf("received:%s\r\n", scanner.Text())
	}
	requireNoError(t, scanner.Err())
	os.Exit(0)
}

func clientLaunchTimeouts() tuitest.Timeouts {
	return tuitest.Timeouts{
		Text: tuitest.Ptr(11 * time.Millisecond), Idle: tuitest.Ptr(12 * time.Millisecond),
		Command: tuitest.Ptr(13 * time.Millisecond), Exit: tuitest.Ptr(14 * time.Millisecond),
		Ready: tuitest.Ptr(15 * time.Millisecond),
	}
}

type launchTimeoutCase struct {
	name     string
	options  tuitest.Timeouts
	expected tuitest.EffectiveTimeouts
}

func launchTimeoutCases() []launchTimeoutCase {
	zero := time.Duration(0)
	return []launchTimeoutCase{
		{name: "inherit", expected: tuitest.EffectiveTimeouts{Text: 11 * time.Millisecond, Idle: 12 * time.Millisecond, Command: 13 * time.Millisecond, Exit: 14 * time.Millisecond, Ready: 15 * time.Millisecond}},
		{name: "partial override", options: tuitest.Timeouts{Text: &zero, Ready: tuitest.Ptr(42 * time.Millisecond)}, expected: tuitest.EffectiveTimeouts{Idle: 12 * time.Millisecond, Command: 13 * time.Millisecond, Exit: 14 * time.Millisecond, Ready: 42 * time.Millisecond}},
		{name: "explicit zero", options: tuitest.Timeouts{Text: &zero, Idle: &zero, Command: &zero, Exit: &zero, Ready: &zero}},
	}
}

func TestClientTimeoutDefaultsReachLaunch(t *testing.T) {
	for _, launch := range []string{"open", "run"} {
		for _, testCase := range launchTimeoutCases() {
			t.Run(launch+"/"+testCase.name, func(t *testing.T) { verifyLaunchTimeouts(t, launch, testCase) })
		}
	}
}

func verifyLaunchTimeouts(t *testing.T, launch string, testCase launchTimeoutCase) {
	t.Helper()
	terminal := newClient(t, tuitest.ClientOptions{Timeouts: clientLaunchTimeouts()})
	options := tuitest.SpawnOptions{WaitReady: tuitest.Ptr(false), Timeouts: testCase.options}
	if launch == "run" {
		runTerminal(t, terminal, options)
	} else {
		_, err := terminal.Open(tuitest.OpenOptions{SpawnOptions: options})
		requireNoError(t, err)
	}
	state, err := terminal.State()
	requireNoError(t, err)
	if state.Timeouts != testCase.expected {
		t.Fatalf("launch timeouts=%+v, expected %+v", state.Timeouts, testCase.expected)
	}
}

func TestInvalidLaunchRetryPreservesSharedSession(t *testing.T) {
	for _, launch := range []string{"open", "run"} {
		t.Run(launch, func(t *testing.T) {
			terminal := newTerminal(t, tuitest.ClientOptions{})
			peer, err := tuitest.New(terminal.Session(), tuitest.ClientOptions{})
			requireNoError(t, err)
			options := tuitest.SpawnOptions{Backend: tuitest.Backend("unknown"), Retries: 1, Restart: tuitest.Ptr(false)}
			if launch == "run" {
				_, err = peer.Run("ignored", nil, options)
			} else {
				_, err = peer.Open(tuitest.OpenOptions{SpawnOptions: options})
			}
			requireKind(t, err, tuitest.UsageError)
			requireNoError(t, terminal.GetByText("go-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(time.Duration(0))}))
			requireNoError(t, terminal.Submit(tuitest.Ptr("session-survived")))
			requireNoError(t, peer.GetByText("received:session-survived", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(time.Second)}))
		})
	}
}

func TestLaunchRetriesReadinessFailure(t *testing.T) {
	directory := t.TempDir()
	terminal := newClient(t, tuitest.ClientOptions{})
	executable, err := os.Executable()
	requireNoError(t, err)
	marker := filepath.Join(directory, "first-launch")
	result, err := terminal.Run(executable, []string{"-test.run=^TestRetryTerminalProcess$", "--", "--tui-go-retry-child"}, tuitest.SpawnOptions{
		Cwd: directory, WaitReady: tuitest.Ptr(true), Retries: 1,
		Timeouts: tuitest.Timeouts{Ready: tuitest.Ptr(3 * time.Second)},
	})
	requireNoError(t, err)
	if !result.Ready {
		t.Fatal("retry returned before the second launch was ready")
	}
	_, err = os.Stat(marker)
	requireNoError(t, err)
	requireNoError(t, terminal.Submit(tuitest.Ptr("retried")))
	requireNoError(t, terminal.GetByText("received:retried", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(3 * time.Second)}))
}
