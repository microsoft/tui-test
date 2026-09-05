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
	fmt.Println("Go binding smoke passed")
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
	return nil
}
