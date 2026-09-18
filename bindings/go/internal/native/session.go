package native

import (
	"fmt"
	"time"
)

type Session struct {
	name      string
	recording *AutomaticRecording
}

func NewSession(name string, recording *AutomaticRecording) (*Session, error) {
	if err := checkNativeVersion(); err != nil {
		return nil, err
	}
	return &Session{name: name, recording: recording}, nil
}

func checkNativeVersion() error {
	if err := loadNativeEngine(); err != nil {
		return &Error{Kind: InternalError, Message: err.Error()}
	}
	if version := nativeFunctions.AbiVersion(); version != 1 {
		return &Error{Kind: InternalError, Message: fmt.Sprintf("native ABI version %d is incompatible with required version 1", version)}
	}
	return nil
}

func Sessions() ([]string, error) {
	if err := checkNativeVersion(); err != nil {
		return nil, err
	}
	result := nativeFunctions.Sessions()
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return nil, err
	}
	return nativeSessionNames(result), nil
}

func CloseAll() error {
	if err := checkNativeVersion(); err != nil {
		return err
	}
	result := nativeFunctions.CloseAll()
	defer nativeFunctions.ResultFree(result)
	return nativeError(result)
}

func Recording(session string) (string, error) {
	if err := checkNativeVersion(); err != nil {
		return "", err
	}
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.Recording(memory.text(session))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return "", err
	}
	return nativeText(result.text), nil
}

func (runtime *Session) Open(options OpenOptions) (OpenResult, error) {
	memory := nativeMemory{}
	defer memory.release()
	converted, err := memory.openOptions(options, runtime.recording)
	if err != nil {
		return OpenResult{}, err
	}
	memory.pinner.Pin(&converted)
	result := nativeFunctions.Open(memory.text(runtime.name), &converted)
	defer nativeFunctions.ResultFree(result)
	return readOpenResult(result)
}

func (runtime *Session) Run(program string, args []string, options SpawnOptions) (OpenResult, error) {
	memory := nativeMemory{}
	defer memory.release()
	converted, err := memory.openOptions(OpenOptions{SpawnOptions: options}, runtime.recording)
	if err != nil {
		return OpenResult{}, err
	}
	arguments, length := memory.strings(args)
	memory.pinner.Pin(&converted)
	result := nativeFunctions.Run(memory.text(runtime.name), &converted, memory.text(program), arguments, length)
	defer nativeFunctions.ResultFree(result)
	return readOpenResult(result)
}

func (runtime *Session) State() (State, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.State(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return State{}, err
	}
	return nativeState(result.state), nil
}

func (runtime *Session) Text(full bool) (string, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.Text(memory.text(runtime.name), full)
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return "", err
	}
	return nativeText(result.text), nil
}

func (runtime *Session) Cells(column, row, width, height uint16) ([]Cell, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.Cells(memory.text(runtime.name), column, row, width, height)
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return nil, err
	}
	return nativeCells(result), nil
}

func (runtime *Session) Resize(cols, rows uint16) error {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.Resize(memory.text(runtime.name), cols, rows)
	defer nativeFunctions.ResultFree(result)
	return nativeError(result)
}

func (runtime *Session) Submit(text *string) error {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.Submit(memory.text(runtime.name), memory.optionalText(text))
	defer nativeFunctions.ResultFree(result)
	return nativeError(result)
}

func (runtime *Session) Key(keys []string, action uint32) error {
	memory := nativeMemory{}
	defer memory.release()
	values, length := memory.strings(keys)
	result := nativeFunctions.Key(memory.text(runtime.name), values, length, action)
	defer nativeFunctions.ResultFree(result)
	return nativeError(result)
}

func (runtime *Session) GetCursor() (Cursor, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.GetCursor(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return Cursor{}, err
	}
	return Cursor{X: result.cursor.x, Y: result.cursor.y}, nil
}

func (runtime *Session) GetSize() (Size, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.GetSize(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return Size{}, err
	}
	return Size{Cols: result.size.cols, Rows: result.size.rows}, nil
}

func (runtime *Session) GetExitCode() (*int32, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.GetExitCode(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return nil, err
	}
	return nativeInt(result.exitCode), nil
}

func (runtime *Session) GetBellCount() (uint64, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.GetBellCount(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return 0, err
	}
	return result.number, nil
}

func (runtime *Session) GetBellEvents() ([]BellEvent, error) {
	memory := nativeMemory{}
	defer memory.release()
	result := nativeFunctions.GetBellEvents(memory.text(runtime.name))
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return nil, err
	}
	return nativeBellEvents(result), nil
}

func (runtime *Session) read(call func(abiString, *nativeMemory) *abiResult, read func(*abiResult)) error {
	memory := nativeMemory{}
	defer memory.release()
	result := call(memory.text(runtime.name), &memory)
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return err
	}
	read(result)
	return nil
}

func (runtime *Session) unit(call func(abiString, *nativeMemory) *abiResult) error {
	return runtime.read(call, func(*abiResult) {})
}

func (runtime *Session) optionalTextResult(call func(abiString, *nativeMemory) *abiResult) (*string, error) {
	var text *string
	err := runtime.read(call, func(result *abiResult) { text = nativeOptionalText(result.text) })
	return text, err
}

func (runtime *Session) textResult(call func(abiString, *nativeMemory) *abiResult) (string, error) {
	var text string
	err := runtime.read(call, func(result *abiResult) { text = nativeText(result.text) })
	return text, err
}

func (runtime *Session) Close() error {
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.Close(session) })
}

func (runtime *Session) Write(text string) error {
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.Write(session, memory.text(text))
	})
}

func (runtime *Session) TypeText(text string) error { return runtime.Write(text) }

func (runtime *Session) Signal(text string) error {
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.Signal(session, memory.text(text))
	})
}

func (runtime *Session) GetCommand() (*string, error) {
	return runtime.optionalTextResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.GetCommand(session) })
}

func (runtime *Session) GetOutput() (*string, error) {
	return runtime.optionalTextResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.GetOutput(session) })
}

func (runtime *Session) GetCwd() (*string, error) {
	return runtime.optionalTextResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.GetCwd(session) })
}

func (runtime *Session) GetTitle() (*string, error) {
	return runtime.optionalTextResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.GetTitle(session) })
}

func (runtime *Session) GetClipboard() (string, error) {
	return runtime.textResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.GetClipboard(session) })
}

func (runtime *Session) StopRecording() (string, error) {
	return runtime.textResult(func(session abiString, _ *nativeMemory) *abiResult { return nativeFunctions.StopRecording(session) })
}

func (runtime *Session) WaitIdle(timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.WaitIdle(session, duration)
	})
}

func (runtime *Session) WaitCommand(timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.WaitCommand(session, duration)
	})
}

func (runtime *Session) WaitExit(timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.WaitExit(session, duration)
	})
}

func (runtime *Session) WaitReady(timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.WaitReady(session, duration)
	})
}

func (runtime *Session) WaitBell(timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.WaitBell(session, duration)
	})
}

func (runtime *Session) WaitTitle(text string, options TitleOptions) error {
	duration, err := nativeDuration(options.Timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.WaitTitle(session, memory.text(text), abiWaitOptions{timeoutMS: duration, regex: options.Regex, not: options.Not})
	})
}

func (runtime *Session) ExpectTitle(text string, options TitleOptions) error {
	duration, err := nativeDuration(options.Timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.ExpectTitle(session, memory.text(text), abiWaitOptions{timeoutMS: duration, regex: options.Regex, not: options.Not})
	})
}

func (runtime *Session) WaitClipboard(options ClipboardWaitOptions) error {
	duration, err := nativeDuration(options.Timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.WaitClipboard(session, memory.optionalText(options.Text), abiWaitOptions{timeoutMS: duration, regex: options.Regex})
	})
}

func (runtime *Session) ExpectExitCode(code int32, timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.ExpectExitCode(session, code, duration)
	})
}

func (runtime *Session) ExpectOutput(text string, regex bool) error {
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.ExpectOutput(session, memory.text(text), regex)
	})
}

func (runtime *Session) ExpectBellCount(count uint64, timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.ExpectBellCount(session, count, duration)
	})
}

func (runtime *Session) Screenshot(path string, options ScreenshotOptions) (string, error) {
	return runtime.textResult(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.Screenshot(session, options.Full, memory.nonemptyText(path), nativeFloat(options.Zoom))
	})
}

func (runtime *Session) StartRecording(path string, options RecordingOptions) error {
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.StartRecording(session, abiRecordingOptions{path: memory.text(path), format: memory.nonemptyText(string(options.Format)), fps: nativeUint(options.FPS), speed: nativeFloat(options.Speed), idleTimeLimit: nativeFloat(options.IdleTimeLimit), zoom: nativeFloat(options.Zoom)})
	})
}

func (runtime *Session) Snapshot(name string, options SnapshotOptions) (SnapshotResult, error) {
	var snapshot SnapshotResult
	err := runtime.read(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.Snapshot(session, memory.text(name), options.Update, options.IncludeColors, options.IncludeTitle, memory.nonemptyText(options.Cwd))
	}, func(result *abiResult) {
		switch result.snapshot {
		case 1:
			snapshot = SnapshotWritten
		case 2:
			snapshot = SnapshotUpdated
		default:
			snapshot = SnapshotPassed
		}
	})
	return snapshot, err
}

func (runtime *Session) MouseClick(options MouseClickOptions) error {
	mouse, err := nativeMouse(options.MouseButtonOptions)
	if err != nil {
		return err
	}
	clicks, err := nativeClicks(options.Clicks)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.MouseClick(session, nativeUint(options.X), nativeUint(options.Y), memory.optionalText(options.OnText), mouse, clicks)
	})
}

func (runtime *Session) MouseMove(column, row uint16) error {
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.MouseMove(session, column, row)
	})
}

func (runtime *Session) MouseDown(column, row uint16, options MouseButtonOptions) error {
	mouse, err := nativeMouse(options)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.MouseDown(session, column, row, mouse)
	})
}

func (runtime *Session) MouseUp(column, row uint16, options MouseButtonOptions) error {
	mouse, err := nativeMouse(options)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.MouseUp(session, column, row, mouse)
	})
}

func (runtime *Session) MouseDrag(x1, y1, x2, y2 uint16, options MouseButtonOptions) error {
	mouse, err := nativeMouse(options)
	if err != nil {
		return err
	}
	return runtime.unit(func(session abiString, _ *nativeMemory) *abiResult {
		return nativeFunctions.MouseDrag(session, x1, y1, x2, y2, mouse)
	})
}

func (runtime *Session) MouseScroll(direction ScrollDirection, amount uint32) error {
	if amount > 65535 {
		return &Error{Kind: UsageError, Message: "scroll amount must be between 0 and 65535"}
	}
	convertedAmount := uint16(amount)
	return runtime.unit(func(session abiString, memory *nativeMemory) *abiResult {
		return nativeFunctions.MouseScroll(session, memory.text(string(direction)), convertedAmount)
	})
}

func (runtime *Session) FindLocator(stages []LocatorStage) ([]TextMatch, error) {
	memory := nativeMemory{}
	defer memory.release()
	query, err := memory.query(stages)
	if err != nil {
		return nil, err
	}
	result := nativeFunctions.FindLocator(memory.text(runtime.name), query)
	defer nativeFunctions.ResultFree(result)
	if err := nativeError(result); err != nil {
		return nil, err
	}
	return nativeMatches(result), nil
}

func (runtime *Session) locatorUnit(stages []LocatorStage, call func(abiString, abiQuery) *abiResult) error {
	memory := nativeMemory{}
	defer memory.release()
	query, err := memory.query(stages)
	if err != nil {
		return err
	}
	result := call(memory.text(runtime.name), query)
	defer nativeFunctions.ResultFree(result)
	return nativeError(result)
}

func (runtime *Session) WaitLocator(stages []LocatorStage, hidden bool, timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.locatorUnit(stages, func(session abiString, query abiQuery) *abiResult {
		return nativeFunctions.WaitLocator(session, query, abiWaitOptions{timeoutMS: duration, not: hidden})
	})
}

func (runtime *Session) ExpectLocator(stages []LocatorStage, options LocatorExpectOptions) error {
	return runtime.WaitLocator(stages, options.Not, options.Timeout)
}

func (runtime *Session) ClickLocator(stages []LocatorStage, options LocatorClickOptions) error {
	duration, err := nativeDuration(options.Timeout)
	if err != nil {
		return err
	}
	mouse, err := nativeMouse(options.MouseButtonOptions)
	if err != nil {
		return err
	}
	clicks, err := nativeClicks(options.Clicks)
	if err != nil {
		return err
	}
	return runtime.locatorUnit(stages, func(session abiString, query abiQuery) *abiResult {
		return nativeFunctions.ClickLocator(session, query, mouse, clicks, duration)
	})
}

func (runtime *Session) HighlightLocator(stages []LocatorStage, timeout *time.Duration) error {
	duration, err := nativeDuration(timeout)
	if err != nil {
		return err
	}
	return runtime.locatorUnit(stages, func(session abiString, query abiQuery) *abiResult {
		return nativeFunctions.HighlightLocator(session, query, duration)
	})
}
