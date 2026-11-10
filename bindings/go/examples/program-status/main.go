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
		fmt.Print("\x1b]7501;state=working:app=demo\x07\x1b]7501;state=blocked:id=build:kind=permission:progress=50:title=QnVpbGQ=\x07")
		_, _ = io.Copy(io.Discard, os.Stdin)
		return
	}
	terminal, err := tuitest.Ephemeral("program-status", tuitest.ClientOptions{})
	check(err)
	defer terminal.CloseQuiet()
	program, err := os.Executable()
	check(err)
	_, err = terminal.Run(program, []string{"--child"}, tuitest.SpawnOptions{WaitReady: tuitest.Ptr(false)})
	check(err)
	check(terminal.WaitProgramStatus(tuitest.ProgramStatusMatch{ID: "build", State: tuitest.Ptr(tuitest.ProgramBlocked)}, tuitest.WaitOptions{}))
	check(terminal.ExpectProgramStatus(tuitest.ProgramStatusMatch{ID: "build", App: tuitest.Ptr("demo"), Title: tuitest.Ptr("Build")}, tuitest.WaitOptions{}))
	statuses, err := terminal.GetProgramStatuses()
	check(err)
	for _, status := range statuses {
		fmt.Printf("id=%q state=%s app=%s\n", status.ID, status.State, *status.App)
	}
	missing, err := terminal.GetProgramStatus("missing")
	check(err)
	fmt.Printf("missing: %v\n", missing)
}
