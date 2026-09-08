package tuitest

import (
	"encoding/json"
	"errors"
	"time"

	"github.com/microsoft/tui-test/bindings/go/internal/native"
)

type nativeRuntime struct{ session *native.Session }

func newNativeRuntime(name string, options ClientOptions) (*nativeRuntime, error) {
	var nativeRecording *native.AutomaticRecording
	if options.Recording != nil {
		nativeRecording = &native.AutomaticRecording{Mode: native.RecordingMode(options.Recording.Mode), Directory: options.Recording.Directory}
	}
	var artifacts *native.ArtifactOptions
	if options.Artifacts != nil {
		artifacts = &native.ArtifactOptions{Directory: options.Artifacts.Dir, Mode: string(options.Artifacts.OnFailure), IncludeRecording: options.Artifacts.IncludeRecording}
	}
	var trace *native.TraceOptions
	if options.Trace != nil {
		trace = &native.TraceOptions{Mode: string(options.Trace.Mode), Directory: options.Trace.Directory}
	}
	session, err := native.NewSession(name, native.SessionOptions{Recording: nativeRecording, Artifacts: artifacts, Trace: trace, ScreenHistoryLimit: options.ScreenHistoryLimit})
	if err != nil {
		return nil, engineError(err)
	}
	return &nativeRuntime{session: session}, nil
}
func engineError(err error) error {
	if err == nil {
		return nil
	}
	var failure *native.Error
	if errors.As(err, &failure) {
		converted := &Error{Kind: ErrorKind(failure.Kind), Message: failure.Message}
		if len(failure.DetailsJSON) != 0 && json.Unmarshal(failure.DetailsJSON, &converted.Details) != nil {
			return &Error{Kind: InternalError, Message: "decode native failure details"}
		}
		if len(failure.ArtifactJSON) != 0 && json.Unmarshal(failure.ArtifactJSON, &converted.Artifact) != nil {
			return &Error{Kind: InternalError, Message: "decode native failure artifact"}
		}
		return converted
	}
	return &Error{Kind: InternalError, Message: err.Error()}
}
func engineTimeoutMilliseconds(timeout time.Duration) (uint64, error) {
	value, err := native.Milliseconds(timeout)
	return value, engineError(err)
}
func nativeSessions() ([]string, error) {
	value, err := native.Sessions()
	return value, engineError(err)
}
func nativeCloseAll() error { return engineError(native.CloseAll()) }
func nativeRecording(session string) (string, error) {
	value, err := native.Recording(session)
	return value, engineError(err)
}
func (runtime *nativeRuntime) close(failed *bool) error {
	return engineError(runtime.session.Close(failed))
}
func (runtime *nativeRuntime) restart(gracefulTimeout time.Duration) (OpenResult, error) {
	value, err := runtime.session.Restart(gracefulTimeout)
	return OpenResult(value), engineError(err)
}
func (runtime *nativeRuntime) typeText(text string) error {
	return engineError(runtime.session.TypeText(text))
}
func (runtime *nativeRuntime) write(text string) error {
	return engineError(runtime.session.Write(text))
}
func (runtime *nativeRuntime) submit(text *string) error {
	return engineError(runtime.session.Submit(text))
}
func (runtime *nativeRuntime) signal(name string) error {
	return engineError(runtime.session.Signal(name))
}
func (runtime *nativeRuntime) resize(cols, rows uint16) error {
	return engineError(runtime.session.Resize(cols, rows))
}
func (runtime *nativeRuntime) key(keys []string, action uint32) error {
	return engineError(runtime.session.Key(keys, action))
}
func (runtime *nativeRuntime) waitIdle(timeout *time.Duration) error {
	return engineError(runtime.session.WaitIdle(timeout))
}
func (runtime *nativeRuntime) waitCommand(timeout *time.Duration) error {
	return engineError(runtime.session.WaitCommand(timeout))
}
func (runtime *nativeRuntime) waitExit(timeout *time.Duration) error {
	return engineError(runtime.session.WaitExit(timeout))
}
func (runtime *nativeRuntime) waitReady(timeout *time.Duration) error {
	return engineError(runtime.session.WaitReady(timeout))
}
func (runtime *nativeRuntime) waitBell(timeout *time.Duration) error {
	return engineError(runtime.session.WaitBell(timeout))
}
func (runtime *nativeRuntime) waitTitle(text string, options TitleOptions) error {
	return engineError(runtime.session.WaitTitle(text, native.TitleOptions(options)))
}
func (runtime *nativeRuntime) expectTitle(text string, options TitleOptions) error {
	return engineError(runtime.session.ExpectTitle(text, native.TitleOptions(options)))
}
func (runtime *nativeRuntime) waitClipboard(options ClipboardWaitOptions) error {
	return engineError(runtime.session.WaitClipboard(native.ClipboardWaitOptions(options)))
}
func (runtime *nativeRuntime) expectExitCode(code int32, timeout *time.Duration) error {
	return engineError(runtime.session.ExpectExitCode(code, timeout))
}
func (runtime *nativeRuntime) expectOutput(text string, regex bool) error {
	return engineError(runtime.session.ExpectOutput(text, regex))
}
func (runtime *nativeRuntime) expectBellCount(count uint64, timeout *time.Duration) error {
	return engineError(runtime.session.ExpectBellCount(count, timeout))
}
func (runtime *nativeRuntime) startRecording(path string, options RecordingOptions) error {
	return engineError(runtime.session.StartRecording(path, engineRecordingOptions(options)))
}
func (runtime *nativeRuntime) mouseClick(options MouseClickOptions) error {
	return engineError(runtime.session.MouseClick(engineMouseClickOptions(options)))
}
func (runtime *nativeRuntime) mouseMove(column, row uint16) error {
	return engineError(runtime.session.MouseMove(column, row))
}
func (runtime *nativeRuntime) mouseDown(column, row uint16, options MouseButtonOptions) error {
	return engineError(runtime.session.MouseDown(column, row, engineMouseOptions(options)))
}
func (runtime *nativeRuntime) mouseUp(column, row uint16, options MouseButtonOptions) error {
	return engineError(runtime.session.MouseUp(column, row, engineMouseOptions(options)))
}
func (runtime *nativeRuntime) mouseDrag(fromColumn, fromRow, toColumn, toRow uint16, options MouseButtonOptions) error {
	return engineError(runtime.session.MouseDrag(fromColumn, fromRow, toColumn, toRow, engineMouseOptions(options)))
}
func (runtime *nativeRuntime) mouseScroll(direction ScrollDirection, amount uint32) error {
	return engineError(runtime.session.MouseScroll(native.ScrollDirection(direction), amount))
}
func (runtime *nativeRuntime) waitLocator(query locatorQuery, hidden bool, timeout *time.Duration) error {
	return engineError(runtime.session.WaitLocator(engineLocatorQuery(query), hidden, timeout))
}
func (runtime *nativeRuntime) expectLocator(query locatorQuery, options LocatorExpectOptions) error {
	return engineError(runtime.session.ExpectLocator(engineLocatorQuery(query), native.LocatorExpectOptions(options)))
}
func (runtime *nativeRuntime) clickLocator(query locatorQuery, options LocatorClickOptions) error {
	return engineError(runtime.session.ClickLocator(engineLocatorQuery(query), engineLocatorClickOptions(options)))
}
func (runtime *nativeRuntime) highlightLocator(query locatorQuery, timeout *time.Duration) error {
	return engineError(runtime.session.HighlightLocator(engineLocatorQuery(query), timeout))
}
func (runtime *nativeRuntime) open(options OpenOptions) (OpenResult, error) {
	value, err := runtime.session.Open(engineOpenOptions(options))
	return OpenResult(value), engineError(err)
}
func (runtime *nativeRuntime) run(program string, args []string, options SpawnOptions) (OpenResult, error) {
	value, err := runtime.session.Run(program, args, engineSpawnOptions(options))
	return OpenResult(value), engineError(err)
}
func (runtime *nativeRuntime) state() (State, error) {
	value, err := runtime.session.State()
	return engineState(value), engineError(err)
}
func (runtime *nativeRuntime) text(full bool) (string, error) {
	value, err := runtime.session.Text(full)
	return value, engineError(err)
}
func (runtime *nativeRuntime) cells(column, row, width, height uint16) ([]Cell, error) {
	value, err := runtime.session.Cells(column, row, width, height)
	return engineCells(value), engineError(err)
}
func (runtime *nativeRuntime) getCursor() (Cursor, error) {
	value, err := runtime.session.GetCursor()
	return Cursor(value), engineError(err)
}
func (runtime *nativeRuntime) getSize() (Size, error) {
	value, err := runtime.session.GetSize()
	return Size(value), engineError(err)
}
func (runtime *nativeRuntime) getExitCode() (*int32, error) {
	value, err := runtime.session.GetExitCode()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getBellCount() (uint64, error) {
	value, err := runtime.session.GetBellCount()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getBellEvents() ([]BellEvent, error) {
	value, err := runtime.session.GetBellEvents()
	return engineBellEvents(value), engineError(err)
}
func (runtime *nativeRuntime) getCommand() (*string, error) {
	value, err := runtime.session.GetCommand()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getOutput() (*string, error) {
	value, err := runtime.session.GetOutput()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getCwd() (*string, error) {
	value, err := runtime.session.GetCwd()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getTitle() (*string, error) {
	value, err := runtime.session.GetTitle()
	return value, engineError(err)
}
func (runtime *nativeRuntime) getClipboard() (string, error) {
	value, err := runtime.session.GetClipboard()
	return value, engineError(err)
}
func (runtime *nativeRuntime) stopRecording() (string, error) {
	value, err := runtime.session.StopRecording()
	return value, engineError(err)
}
func (runtime *nativeRuntime) screenshot(path string, options ScreenshotOptions) (string, error) {
	value, err := runtime.session.Screenshot(path, native.ScreenshotOptions{Full: options.Full, Zoom: options.Zoom, Background: options.Background, Transparent: options.Transparent})
	return value, engineError(err)
}
func (runtime *nativeRuntime) snapshot(name string, options SnapshotOptions) (SnapshotResult, error) {
	value, err := runtime.session.Snapshot(name, native.SnapshotOptions(options))
	return SnapshotResult(value), engineError(err)
}
func (runtime *nativeRuntime) findLocator(query locatorQuery, requireOne bool) ([]TextMatch, error) {
	value, err := runtime.session.FindLocator(engineLocatorQuery(query), requireOne)
	return engineMatches(value), engineError(err)
}

func engineSpawnOptions(options SpawnOptions) native.SpawnOptions {
	var profile *native.Profile
	if options.Profile != nil {
		profile = &native.Profile{Scrollback: options.Profile.Scrollback, Colors: native.Colors(options.Profile.Colors)}
	}
	return native.SpawnOptions{Backend: native.Backend(options.Backend), Cols: options.Cols, Rows: options.Rows, Cwd: options.Cwd, Env: options.Env, WaitReady: options.WaitReady, Restart: options.Restart, Profile: profile, Timeouts: native.Timeouts(options.Timeouts)}
}
func engineOpenOptions(options OpenOptions) native.OpenOptions {
	return native.OpenOptions{SpawnOptions: engineSpawnOptions(options.SpawnOptions), Shell: native.Shell(options.Shell)}
}
func engineRecordingOptions(options RecordingOptions) native.RecordingOptions {
	return native.RecordingOptions{Format: native.RecordingFormat(options.Format), FPS: options.FPS, Speed: options.Speed, IdleTimeLimit: options.IdleTimeLimit, Zoom: options.Zoom, Background: options.Background, Transparent: options.Transparent}
}
func engineMouseOptions(options MouseButtonOptions) native.MouseButtonOptions {
	return native.MouseButtonOptions{Button: native.MouseButton(options.Button), Alt: options.Alt, Ctrl: options.Ctrl, Shift: options.Shift}
}
func engineMouseClickOptions(options MouseClickOptions) native.MouseClickOptions {
	return native.MouseClickOptions{MouseButtonOptions: engineMouseOptions(options.MouseButtonOptions), X: options.X, Y: options.Y, OnText: options.OnText, Clicks: options.Clicks}
}
func engineLocatorClickOptions(options LocatorClickOptions) native.LocatorClickOptions {
	return native.LocatorClickOptions{MouseButtonOptions: engineMouseOptions(options.MouseButtonOptions), Clicks: options.Clicks, Timeout: options.Timeout}
}
func engineTextStyle(style TextStyle) native.TextStyle {
	var underline *native.UnderlineStyle
	if style.UnderlineStyle != nil {
		underline = Ptr(native.UnderlineStyle(*style.UnderlineStyle))
	}
	return native.TextStyle{Foreground: style.Foreground, Background: style.Background, Bold: style.Bold, Dim: style.Dim, Italic: style.Italic, UnderlineStyle: underline, UnderlineColor: style.UnderlineColor, Inverse: style.Inverse, Hidden: style.Hidden, Strikethrough: style.Strikethrough, Blink: style.Blink}
}
func engineLocatorQuery(query locatorQuery) native.LocatorExpression {
	converted := make([]native.LocatorNode, len(query.nodes))
	for index, node := range query.nodes {
		converted[index] = native.LocatorNode{Kind: node.kind, Text: node.text, Regex: node.regex, Whitespace: (*native.Whitespace)(node.whitespace), Link: node.link, Full: node.full, Direction: (*native.Direction)(node.direction), Within: node.within, Left: node.left, Right: node.right, Input: node.input, Has: node.has, HasNot: node.hasNot, Occurrence: node.occurrence}
		if node.style != nil {
			style := engineTextStyle(*node.style)
			converted[index].Style = &style
		}
	}
	return native.LocatorExpression{Nodes: converted, Root: query.root}
}
func engineState(state native.State) State {
	return State{SessionShell: state.SessionShell, Cols: state.Cols, Rows: state.Rows, Cursor: Cursor(state.Cursor), Title: state.Title, Cwd: state.Cwd, LastCommand: state.LastCommand, LastExit: state.LastExit, ExitSignal: state.ExitSignal, Exited: state.Exited, Ready: state.Ready, BellCount: state.BellCount, Modes: state.Modes, MouseMode: state.MouseMode, Colors: TerminalColors(state.Colors), Timeouts: EffectiveTimeouts(state.Timeouts), Text: state.Text}
}
func engineCells(cells []native.Cell) []Cell {
	if cells == nil {
		return nil
	}

	converted := make([]Cell, len(cells))
	for index, cell := range cells {
		converted[index] = Cell{X: cell.X, Y: cell.Y, Char: cell.Char, FG: Color(cell.FG), BG: Color(cell.BG), Bold: cell.Bold, Dim: cell.Dim, Italic: cell.Italic, Inverse: cell.Inverse, Invisible: cell.Invisible, Strike: cell.Strike, Blink: cell.Blink, Underline: cell.Underline, UnderlineStyle: UnderlineStyle(cell.UnderlineStyle), UnderlineColor: Color(cell.UnderlineColor), Link: cell.Link, LinkID: cell.LinkID}
	}
	return converted
}
func engineMatches(matches []native.TextMatch) []TextMatch {
	if matches == nil {
		return nil
	}

	converted := make([]TextMatch, len(matches))
	for index, match := range matches {
		spans := make([]TextSpan, len(match.Spans))
		for spanIndex, span := range match.Spans {
			spans[spanIndex] = TextSpan(span)
		}
		converted[index] = TextMatch{Text: match.Text, Start: TextPosition(match.Start), End: TextPosition(match.End), Spans: spans}
	}
	return converted
}
func engineBellEvents(events []native.BellEvent) []BellEvent {
	if events == nil {
		return nil
	}

	converted := make([]BellEvent, len(events))
	for index, event := range events {
		converted[index] = BellEvent(event)
	}
	return converted
}
