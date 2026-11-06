package main

import (
	"bufio"
	"fmt"
	"image/gif"
	"os"
	"path/filepath"
	"strings"
	"time"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--child" {
		fmt.Print("\x1b[5 q\x1b]10;#abcdef\x07rendering-ready")
		bufio.NewScanner(os.Stdin).Scan()
		return
	}
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	directory := "target/binding-parity/go"
	if len(os.Args) > 1 {
		directory = os.Args[1]
	}
	if err := os.MkdirAll(directory, 0o755); err != nil {
		return err
	}
	style := &tuitest.RenderStyle{
		Font: tuitest.FontFamilies{Family: tuitest.Ptr("monospace")}, FontSize: tuitest.Ptr(24.0),
		CanvasPadding:  tuitest.Padding{Uniform: tuitest.Ptr(uint32(0))},
		ContentPadding: tuitest.Padding{Left: tuitest.Ptr(uint32(2))},
		Window:         tuitest.WindowStyle{TitleBar: tuitest.Ptr(false)},
		Border:         tuitest.BorderStyle{Radius: tuitest.Ptr(0.0)},
		Shadow:         tuitest.ShadowStyle{Enabled: tuitest.Ptr(false)},
		Cursor:         tuitest.RecordingCursorStyle{Blink: tuitest.Ptr(false), BlinkIntervalMS: tuitest.Ptr(uint64(250))},
	}
	terminal, err := tuitest.Ephemeral("rendering", tuitest.ClientOptions{Style: style})
	if err != nil {
		return err
	}
	defer terminal.CloseQuiet()
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	if _, err = terminal.Run(executable, []string{"--child"}, tuitest.SpawnOptions{Cols: tuitest.Ptr(uint16(30)), Rows: tuitest.Ptr(uint16(3))}); err != nil {
		return err
	}
	if err = terminal.GetByText("rendering-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}); err != nil {
		return err
	}
	if err = terminal.ExpectCursor(tuitest.CursorExpectation{Visible: tuitest.Ptr(true), Shape: tuitest.Ptr("bar")}); err != nil {
		return err
	}
	if err = terminal.ExpectMode("cursor_visible", true, tuitest.WaitOptions{}); err != nil {
		return err
	}
	if err = terminal.ExpectColors(tuitest.ColorExpectation{Foreground: tuitest.Ptr("#abcdef")}); err != nil {
		return err
	}
	path := filepath.Join(directory, "custom.svg")
	if _, err = terminal.Screenshot(path, tuitest.ScreenshotOptions{}); err != nil {
		return err
	}
	svg, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if !strings.Contains(string(svg), `font-size="24px"`) || !strings.Contains(string(svg), `font-family="monospace"`) {
		return fmt.Errorf("custom style did not reach the renderer")
	}
	if _, err = terminal.Restart(tuitest.RestartOptions{GracefulTimeout: tuitest.Ptr(time.Duration(0))}); err != nil {
		return err
	}
	if err = terminal.GetByText("rendering-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}); err != nil {
		return err
	}
	if _, err = terminal.Screenshot(path, tuitest.ScreenshotOptions{}); err != nil {
		return err
	}
	restarted, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if string(restarted) != string(svg) {
		return fmt.Errorf("restart changed the rendering style")
	}
	recording := filepath.Join(directory, "steady.gif")
	if err = terminal.StartRecording(recording, tuitest.RecordingOptions{}); err != nil {
		return err
	}
	// Set playback duration after the readiness check.
	time.Sleep(1100 * time.Millisecond)
	if _, err = terminal.StopRecording(); err != nil {
		return err
	}
	file, err := os.Open(recording)
	if err != nil {
		return err
	}
	defer file.Close()
	animation, err := gif.DecodeAll(file)
	if err != nil {
		return err
	}
	if len(animation.Image) != 1 {
		return fmt.Errorf("blink=false produced %d frames", len(animation.Image))
	}
	fmt.Printf("%s: custom style survives restart; blink=false produces one GIF frame\n", path)
	fmt.Println("Native cursor, mode, and color assertions passed")
	return nil
}
