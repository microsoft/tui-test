package main

import (
	"fmt"
	"io"
	"os"
	"strings"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--child" {
		fmt.Print("Start\r\n\x1b[1mSave\x1b[0m\r\nEnd\r\n")
		_, _ = io.Copy(io.Discard, os.Stdin)
		return
	}
	directory := os.Getenv("TUI_TEST_PARITY_ARTIFACTS")
	if directory == "" {
		directory = "artifacts/parity-go"
	}
	terminal, err := tuitest.Ephemeral("parity", tuitest.ClientOptions{
		Context: &tuitest.ExecutionContext{DiagnosticContext: map[string]string{"example": "session-parity"}},
	})
	check(err)
	defer terminal.CloseQuiet()
	program, err := os.Executable()
	check(err)
	_, err = terminal.Run(program, []string{"--child"}, tuitest.SpawnOptions{
		WaitReady: tuitest.Ptr(false),
		Recording: &tuitest.AutomaticRecording{Directory: directory},
		Context: &tuitest.ExecutionContext{
			DiagnosticContext: map[string]string{"phase": "spawn"},
			Trace:             &tuitest.TraceOptions{Mode: tuitest.TraceOn, Directory: directory},
		},
	})
	check(err)
	save := terminal.GetByText("Save", tuitest.TextSelectorOptions{Scope: &tuitest.TextScope{
		After: &tuitest.TextAnchor{Text: "Start"}, Before: &tuitest.TextAnchor{Text: "End"},
	}}).WithStyle(tuitest.TextStyle{Bold: tuitest.Ptr(true)})
	check(save.Expect(tuitest.LocatorExpectOptions{}))
	relative, err := save.RelativeTo(terminal.GetByText("Start", tuitest.TextSelectorOptions{}), tuitest.After)
	check(err)
	composite, err := relative.WithStyle(tuitest.TextStyle{Bold: tuitest.Ptr(true)}).Or(
		terminal.GetByLink("test:save", tuitest.LinkSelectorOptions{}))
	check(err)
	matches, err := composite.HighlightMatches(tuitest.WaitOptions{})
	check(err)
	screen, err := terminal.PackedScreen(tuitest.TextOptions{})
	check(err)
	if len(matches) != 1 || matches[0].Text != "Save" || !strings.Contains(string(screen.UTF8), "Save") {
		panic("unexpected locator or screen result")
	}
	check(terminal.FinishTrace(false))
	_, err = terminal.State()
	check(err)
	fmt.Printf("scoped bold matches=%d; packed=%dx%d; trace finished; session still open\n", len(matches), screen.Cols, screen.Rows)
	check(terminal.Interrupt())
	fmt.Println("pending operations cancelled; child terminated")
	check(terminal.Close())
	cast, err := terminal.Run(program, []string{"--child"}, tuitest.SpawnOptions{
		WaitReady: tuitest.Ptr(false),
		Recording: &tuitest.AutomaticRecording{Mode: tuitest.RecordingAlways, Directory: directory + "/cast-only"},
	})
	check(err)
	check(terminal.GetByText("Save", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
	content, err := tuitest.Recording(terminal.Session())
	check(err)
	if !strings.Contains(content, "Save") {
		panic("missing cast-only output")
	}
	fmt.Printf("cast-only recording=%s\n", cast.Recording)
}
