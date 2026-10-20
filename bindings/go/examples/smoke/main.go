// The smoke consumer runs itself in child mode, so it needs no external shell.
package main

import (
	"bufio"
	"fmt"
	"os"
	"time"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--child" {
		fmt.Println("go-binding-ready")
		scanner := bufio.NewScanner(os.Stdin)
		for scanner.Scan() {
			if scanner.Text() == "quit" {
				return
			}
			fmt.Println("echo:" + scanner.Text())
		}
		return
	}
	if err := smoke(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	source := "bundled"
	if os.Getenv("TUI_TEST_NATIVE_LIBRARY") != "" {
		source = "override"
	}
	fmt.Printf("Go binding smoke passed (%s engine)\n", source)
}

func smoke() error {
	terminal, err := tuitest.Ephemeral("smoke", tuitest.ClientOptions{Recording: &tuitest.AutomaticRecording{Mode: tuitest.RecordingDisabled}})
	if err != nil {
		return fmt.Errorf("create smoke terminal: %w", err)
	}
	defer terminal.CloseQuiet()
	executable, err := os.Executable()
	if err != nil {
		return fmt.Errorf("locate smoke executable: %w", err)
	}
	if _, err = terminal.Run(executable, []string{"--child"}, tuitest.SpawnOptions{WaitReady: tuitest.Ptr(false)}); err != nil {
		return fmt.Errorf("run smoke child: %w", err)
	}
	return exerciseTerminal(terminal)
}

func exerciseTerminal(terminal *tuitest.Client) error {
	timeout := 10 * time.Second
	if err := terminal.GetByText("go-binding-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: &timeout}); err != nil {
		return fmt.Errorf("wait for child readiness: %w", err)
	}
	if err := terminal.Submit(tuitest.Ptr("hello")); err != nil {
		return fmt.Errorf("submit child input: %w", err)
	}
	if err := terminal.GetByText("echo:hello", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: &timeout}); err != nil {
		return fmt.Errorf("expect child response: %w", err)
	}
	if err := terminal.Submit(tuitest.Ptr("quit")); err != nil {
		return fmt.Errorf("request child exit: %w", err)
	}
	if err := terminal.WaitExit(tuitest.WaitOptions{Timeout: &timeout}); err != nil {
		return fmt.Errorf("wait for child exit: %w", err)
	}
	state, err := terminal.State()
	if err != nil {
		return fmt.Errorf("read child exit status: %w", err)
	}
	if state.Exited == nil {
		return fmt.Errorf("child exit status is unavailable")
	}
	if *state.Exited != 0 {
		return fmt.Errorf("child exited with status %d", *state.Exited)
	}
	return nil
}
