package main

import (
	"fmt"
	"io"
	"os"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--child" {
		fmt.Print("\x1b]9;4;1;50\x07")
		_, _ = io.Copy(io.Discard, os.Stdin)
		return
	}
	terminal, err := tuitest.Ephemeral("progress-bar", tuitest.ClientOptions{})
	check(err)
	defer terminal.CloseQuiet()
	program, err := os.Executable()
	check(err)
	_, err = terminal.Run(program, []string{"--child"}, tuitest.SpawnOptions{WaitReady: tuitest.Ptr(false)})
	check(err)
	check(terminal.WaitProgressBar(tuitest.ProgressBarMatch{State: tuitest.Ptr(tuitest.ProgressNormal)}, tuitest.WaitOptions{}))
	check(terminal.ExpectProgressBar(tuitest.ProgressBarMatch{Progress: tuitest.Ptr(uint8(50))}, tuitest.WaitOptions{}))
	progress, err := terminal.GetProgressBar()
	check(err)
	fmt.Printf("state=%s progress=%d\n", progress.State, *progress.Progress)
}
