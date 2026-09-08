package tuitest_test

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	tuitest "github.com/microsoft/tui-test/bindings/go"
)

func TestTerminalProcess(t *testing.T) {
	if !slices.Contains(os.Args, "--tui-go-child") {
		return
	}
	fmt.Print("\x1b[2J\x1b[Hgo-ready\r\nitem item\r\n\x1b[1mstyled\x1b[0m plain\r\n界 café\r\n")
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		switch scanner.Text() {
		case "quit":
			os.Exit(0)
		case "events":
			fmt.Print("\x1b]2;Go title\x07\x1b]52;c;Z28tY2xpcGJvYXJk\x07\x07\r\nevents-done\r\n")
		default:
			fmt.Printf("received:%s\r\n", scanner.Text())
		}
	}
	os.Exit(0)
}

func newTerminal(t *testing.T, options tuitest.ClientOptions) *tuitest.Client {
	t.Helper()
	terminal := newClient(t, options)
	runTerminal(t, terminal, tuitest.SpawnOptions{})
	return terminal
}

func newClient(t *testing.T, options tuitest.ClientOptions) *tuitest.Client {
	t.Helper()
	if options.Recording == nil {
		options.Recording = &tuitest.AutomaticRecording{Mode: tuitest.RecordingDisabled}
	}
	terminal, err := tuitest.Ephemeral(t.Name(), options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := terminal.Close(); err != nil {
			t.Errorf("close terminal: %v", err)
		}
	})
	return terminal
}
func runTerminal(t *testing.T, terminal *tuitest.Client, options tuitest.SpawnOptions) {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	options.WaitReady = tuitest.Ptr(false)
	if _, err = terminal.Run(executable, []string{"-test.run=^TestTerminalProcess$", "--", "--tui-go-child"}, options); err != nil {
		t.Fatal(err)
	}
	if err = terminal.GetByText("go-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{Timeout: tuitest.Ptr(10 * time.Second)}); err != nil {
		t.Fatal(err)
	}
}
func requireNoError(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}
func requireKind(t *testing.T, err error, kind tuitest.ErrorKind) {
	t.Helper()
	var failure *tuitest.Error
	if !errors.As(err, &failure) || failure.Kind != kind {
		t.Fatalf("expected %s error, got %v", kind, err)
	}
}

func TestLocatorsPreserveSelection(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	items := terminal.GetByText("item", tuitest.TextSelectorOptions{})
	count, err := items.Count()
	requireNoError(t, err)
	if count != 2 {
		t.Fatalf("item count=%d", count)
	}
	_, err = items.Location()
	requireKind(t, err, tuitest.AssertionError)
	first, err := items.First().Location()
	requireNoError(t, err)
	last, err := items.Last().Location()
	requireNoError(t, err)
	if first.Start.Column >= last.Start.Column {
		t.Fatalf("unordered locations: %+v %+v", first, last)
	}
	requireNoError(t, items.Highlight(tuitest.WaitOptions{}))
	requireNoError(t, items.First().Click(tuitest.LocatorClickOptions{}))
	requireNoError(t, terminal.GetByText("absent", tuitest.TextSelectorOptions{}).Wait(tuitest.LocatorWaitOptions{State: tuitest.Hidden, Timeout: tuitest.Ptr(time.Duration(0))}))
}

func TestLocatorEnumerationAndRelativeQueries(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	items := terminal.GetByText("item", tuitest.TextSelectorOptions{})
	all, err := items.All()
	requireNoError(t, err)
	for index, locator := range all {
		actual, locationErr := locator.Location()
		requireNoError(t, locationErr)
		selected, selectedErr := items.Nth(uint32(index)).Location()
		requireNoError(t, selectedErr)
		if actual.Start != selected.Start {
			t.Fatalf("all[%d]=%+v; nth=%+v", index, actual, selected)
		}
	}
	count, err := items.Count()
	requireNoError(t, err)
	if count != 2 {
		t.Fatal("selection mutated original locator")
	}
	requireNoError(t, terminal.GetByText("item item", tuitest.TextSelectorOptions{}).GetByText("item", tuitest.TextSelectorOptions{}).Nth(1).Expect(tuitest.LocatorExpectOptions{}))
	requireNoError(t, items.First().GetByText("item", tuitest.TextSelectorOptions{Direction: tuitest.After}).Expect(tuitest.LocatorExpectOptions{}))
}

func TestStylesAndWideCellCoordinates(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	bold := true
	styled := terminal.GetByStyle(tuitest.TextStyle{Bold: &bold}, tuitest.StyleSelectorOptions{})
	bold = false
	requireNoError(t, styled.GetByText("styled", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
	requireNoError(t, terminal.GetByText("plain", tuitest.TextSelectorOptions{}).GetByStyle(tuitest.TextStyle{Bold: tuitest.Ptr(false)}, tuitest.StyleSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
	wide, err := terminal.GetByText("界", tuitest.TextSelectorOptions{}).Location()
	requireNoError(t, err)
	if wide.Start.Column != 0 || wide.Start.Row != 3 {
		t.Fatalf("unexpected wide cell coordinates: %+v", wide.Start)
	}
	cells, err := terminal.Cells(0, 3, 2, 1)
	requireNoError(t, err)
	if len(cells) != 2 || cells[0].Char != "界" || cells[1].Char != "" {
		t.Fatalf("wide cells=%+v", cells)
	}
}

func TestInputAndState(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	state, err := terminal.State()
	requireNoError(t, err)
	if state.Cols != 80 || state.Rows != 30 {
		t.Fatalf("default size=%dx%d", state.Cols, state.Rows)
	}
	requireNoError(t, terminal.Resize(90, 32))
	size, err := terminal.GetSize()
	requireNoError(t, err)
	if size.Cols != 90 || size.Rows != 32 {
		t.Fatalf("resized size=%+v", size)
	}
	_, err = terminal.GetCursor()
	requireNoError(t, err)
	_, err = terminal.GetCommand()
	requireNoError(t, err)
	_, err = terminal.GetOutput()
	requireNoError(t, err)
	_, err = terminal.GetExitCode()
	requireNoError(t, err)
	_, err = terminal.GetCwd()
	requireNoError(t, err)
	requireNoError(t, terminal.Type("hello"))
	requireNoError(t, terminal.Press("Enter"))
	requireNoError(t, terminal.GetByText("received:hello", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
	requireNoError(t, terminal.Submit(tuitest.Ptr("quit")))
	requireNoError(t, terminal.WaitExit(tuitest.WaitOptions{Timeout: tuitest.Ptr(10 * time.Second)}))
}

func TestTerminalEvents(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	requireNoError(t, terminal.Submit(tuitest.Ptr("events")))
	requireNoError(t, terminal.WaitTitle("Go title", tuitest.TitleOptions{}))
	requireNoError(t, terminal.ExpectTitle("^Go", tuitest.TitleOptions{Regex: true}))
	requireNoError(t, terminal.WaitClipboard(tuitest.ClipboardWaitOptions{Text: tuitest.Ptr("go-clipboard")}))
	title, err := terminal.GetTitle()
	requireNoError(t, err)
	if title == nil || *title != "Go title" {
		t.Fatalf("title=%v", title)
	}
	clipboard, err := terminal.GetClipboard()
	requireNoError(t, err)
	if clipboard != "go-clipboard" {
		t.Fatalf("clipboard=%q", clipboard)
	}
	requireNoError(t, terminal.ExpectBellCount(1, tuitest.WaitOptions{}))
	events, err := terminal.GetBellEvents()
	requireNoError(t, err)
	if len(events) != 1 || events[0].Sequence != 1 {
		t.Fatalf("bell events=%+v", events)
	}
	count, err := terminal.GetBellCount()
	requireNoError(t, err)
	if count != 1 {
		t.Fatalf("bell count=%d", count)
	}
}

func TestScreenshotsAndSnapshots(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	text, err := terminal.Screenshot("", tuitest.ScreenshotOptions{})
	requireNoError(t, err)
	if !strings.Contains(text, "go-ready") {
		t.Fatalf("screenshot omitted terminal text: %.100s", text)
	}
	directory := t.TempDir()
	path, err := terminal.Screenshot(filepath.Join(directory, "terminal.svg"), tuitest.ScreenshotOptions{})
	requireNoError(t, err)
	if _, statErr := os.Stat(path); statErr != nil {
		t.Fatal(statErr)
	}
	snapshot, err := terminal.ExpectSnapshot("terminal", tuitest.SnapshotOptions{Cwd: directory, Update: true})
	requireNoError(t, err)
	if snapshot != tuitest.SnapshotWritten && snapshot != tuitest.SnapshotUpdated {
		t.Fatalf("snapshot=%s", snapshot)
	}
	_, err = terminal.ExpectSnapshot("terminal", tuitest.SnapshotOptions{Cwd: directory})
	requireNoError(t, err)
	requireNoError(t, terminal.Submit(tuitest.Ptr("quit")))
	requireNoError(t, terminal.WaitExit(tuitest.WaitOptions{Timeout: tuitest.Ptr(10 * time.Second)}))
}

func TestNamedHandlesReopen(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	peer, err := tuitest.New(terminal.Session(), tuitest.ClientOptions{})
	requireNoError(t, err)
	names, err := tuitest.Sessions()
	requireNoError(t, err)
	if !slices.Contains(names, terminal.Session()) {
		t.Fatalf("missing session in %v", names)
	}
	requireNoError(t, terminal.Close())
	_, err = peer.Text(tuitest.TextOptions{})
	requireKind(t, err, tuitest.NoSessionError)
	runTerminal(t, terminal, tuitest.SpawnOptions{})
	requireNoError(t, peer.GetByText("go-ready", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
}

func TestCloseInterruptsPendingWait(t *testing.T) {
	t.Skip("Known upstream limitation accepted for this binding: named Close blocks behind pending waits; https://github.com/microsoft/tui-test/issues/207")
	terminal := newTerminal(t, tuitest.ClientOptions{})
	verifyWaitInterruption(t, terminal, terminal.Close)
}

func TestCloseAllInterruptsPendingWait(t *testing.T) {
	terminal := newTerminal(t, tuitest.ClientOptions{})
	verifyWaitInterruption(t, terminal, tuitest.CloseAll)
}

func verifyWaitInterruption(t *testing.T, terminal *tuitest.Client, closeSession func() error) {
	t.Helper()
	started := make(chan struct{})
	waitResult := make(chan error, 1)
	go func() {
		close(started)
		waitResult <- terminal.GetByText("never-visible", tuitest.TextSelectorOptions{}).Wait(tuitest.LocatorWaitOptions{Timeout: tuitest.Ptr(5 * time.Second)})
	}()
	<-started
	select {
	case err := <-waitResult:
		t.Fatalf("wait returned before close: %v", err)
	case <-time.After(100 * time.Millisecond):
	}
	closeResult := make(chan error, 1)
	go func() { closeResult <- closeSession() }()
	select {
	case err := <-closeResult:
		requireNoError(t, err)
	case <-time.After(2 * time.Second):
		t.Fatal("close blocked behind the pending wait")
	}
	select {
	case err := <-waitResult:
		if err == nil {
			t.Fatal("interrupted wait unexpectedly succeeded")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("wait did not return after close")
	}
}

func TestTimeoutOptionsAndTypedFailures(t *testing.T) {
	_, err := tuitest.New("", tuitest.ClientOptions{Timeouts: tuitest.Timeouts{Text: tuitest.Ptr(-time.Nanosecond)}})
	requireKind(t, err, tuitest.UsageError)
	terminal := newTerminal(t, tuitest.ClientOptions{Timeouts: tuitest.Timeouts{Text: tuitest.Ptr(time.Duration(0))}, Artifacts: &tuitest.ArtifactOptions{Dir: t.TempDir(), OnFailure: tuitest.ArtifactSVG}})
	err = terminal.GetByText("missing", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{})
	requireKind(t, err, tuitest.AssertionError)
	var failure *tuitest.Error
	if !errors.As(err, &failure) || failure.Terminal == nil || failure.Terminal.Screenshot == "" || !strings.Contains(failure.Message, "Terminal content:") {
		t.Fatalf("missing diagnostic/artifact: %#v", failure)
	}
	_, err = terminal.Screenshot("", tuitest.ScreenshotOptions{Zoom: tuitest.Ptr(2.0)})
	requireKind(t, err, tuitest.UsageError)
	requireNoError(t, terminal.Close())
	runTerminal(t, terminal, tuitest.SpawnOptions{Timeouts: tuitest.Timeouts{Text: tuitest.Ptr(1500 * time.Microsecond), Idle: tuitest.Ptr(time.Duration(0))}})
	state, err := terminal.State()
	requireNoError(t, err)
	if state.Timeouts.Text != 2*time.Millisecond || state.Timeouts.Idle != 0 {
		t.Fatalf("effective timeouts=%+v", state.Timeouts)
	}
}

func TestRecordingAndCloseAll(t *testing.T) {
	directory := t.TempDir()
	terminal := newTerminal(t, tuitest.ClientOptions{Recording: &tuitest.AutomaticRecording{Mode: tuitest.RecordingAlways, Directory: directory}})
	recording, err := tuitest.Recording(terminal.Session())
	requireNoError(t, err)
	if !strings.Contains(recording, `"version":2`) {
		t.Fatal("missing automatic recording content")
	}
	requireNoError(t, terminal.StartRecording(filepath.Join(directory, "explicit.cast"), tuitest.RecordingOptions{Format: tuitest.Cast}))
	requireNoError(t, terminal.Submit(tuitest.Ptr("recorded")))
	requireNoError(t, terminal.GetByText("received:recorded", tuitest.TextSelectorOptions{}).Expect(tuitest.LocatorExpectOptions{}))
	explicit, err := terminal.StopRecording()
	requireNoError(t, err)
	if _, statErr := os.Stat(explicit); statErr != nil {
		t.Fatal(statErr)
	}
	requireNoError(t, tuitest.CloseAll())
	retained, err := tuitest.Recording(terminal.Session())
	requireNoError(t, err)
	if !strings.Contains(retained, "recorded") {
		t.Fatalf("retained recording omitted terminal output: %q", retained)
	}
}
