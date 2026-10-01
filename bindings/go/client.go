package tuitest

import (
	"crypto/rand"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"time"
)

// Client is a named handle. Handles with the same name share a session and can
// be reused after that session closes. Its synchronous methods may run in goroutines.
type Client struct {
	session         string
	options         ClientOptions
	runtime         *nativeRuntime
	Keyboard        *Keyboard
	Mouse           *Mouse
	artifactCounter atomic.Uint64
}

// New resolves an empty session from TUI_TEST_SESSION, then "default".
func New(session string, options ClientOptions) (*Client, error) {
	if session == "" {
		session = os.Getenv("TUI_TEST_SESSION")
	}
	if session == "" {
		session = "default"
	}
	if err := validateTimeouts(options.Timeouts); err != nil {
		return nil, err
	}
	if err := validateClientOptions(options); err != nil {
		return nil, err
	}
	options = cloneClientOptions(options)
	runtime, err := newNativeRuntime(session, options.Recording)
	if err != nil {
		return nil, err
	}
	client := &Client{session: session, options: options, runtime: runtime}
	client.Keyboard = &Keyboard{runtime: runtime}
	client.Mouse = &Mouse{runtime: runtime}
	return client, nil
}

func validateClientOptions(options ClientOptions) error {
	switch options.Backend {
	case "", Alacritty, Ghostty, Rio, XtermJS:
	default:
		return &Error{Kind: UsageError, Message: "unknown backend: " + string(options.Backend)}
	}
	if options.Recording != nil {
		switch options.Recording.Mode {
		case "", RecordingDisabled, RecordingOnFailure, RecordingAlways:
		default:
			return &Error{Kind: UsageError, Message: "unknown recording mode: " + string(options.Recording.Mode)}
		}
	}
	return validateArtifactOptions(options.Artifacts)
}
func validateArtifactOptions(options *ArtifactOptions) error {
	if options != nil {
		switch options.OnFailure {
		case "", ArtifactSVG, ArtifactText, ArtifactNone:
		default:
			return &Error{Kind: UsageError, Message: "unknown artifact mode: " + string(options.OnFailure)}
		}
		if options.OnFailure != ArtifactNone && options.Dir == "" {
			return &Error{Kind: UsageError, Message: "artifact directory must not be empty"}
		}
	}
	return nil
}

// Ephemeral creates a handle with a unique, filename-safe session name.
func Ephemeral(prefix string, options ClientOptions) (*Client, error) {
	if prefix == "" {
		prefix = "go"
	}
	prefix = strings.Map(func(character rune) rune {
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '-' || character == '_' {
			return character
		}
		return '-'
	}, prefix)
	return New(prefix+"-"+rand.Text(), options)
}
func (client *Client) Session() string         { return client.session }
func Sessions() ([]string, error)              { return nativeSessions() }
func CloseAll() error                          { return nativeCloseAll() }
func Recording(session string) (string, error) { return nativeRecording(session) }

func clonePointer[Value any](value *Value) *Value {
	if value == nil {
		return nil
	}
	return Ptr(*value)
}
func cloneTimeouts(options Timeouts) Timeouts {
	return Timeouts{clonePointer(options.Text), clonePointer(options.Idle), clonePointer(options.Command), clonePointer(options.Exit), clonePointer(options.Ready)}
}
func cloneClientOptions(options ClientOptions) ClientOptions {
	options.Timeouts = cloneTimeouts(options.Timeouts)
	options.Profile = clonePointer(options.Profile)
	if options.Profile != nil {
		options.Profile.Scrollback = clonePointer(options.Profile.Scrollback)
	}
	options.Recording = clonePointer(options.Recording)
	options.Artifacts = clonePointer(options.Artifacts)
	return options
}
func validateTimeouts(options Timeouts) error {
	for _, timeout := range []*time.Duration{options.Text, options.Idle, options.Command, options.Exit, options.Ready} {
		if timeout == nil {
			continue
		}
		if _, err := timeoutMilliseconds(*timeout); err != nil {
			return err
		}
	}
	return nil
}
func timeoutMilliseconds(timeout time.Duration) (uint64, error) {
	return engineTimeoutMilliseconds(timeout)
}

func withTimeoutDefaults(options, defaults Timeouts) Timeouts {
	if options.Text == nil {
		options.Text = defaults.Text
	}
	if options.Idle == nil {
		options.Idle = defaults.Idle
	}
	if options.Command == nil {
		options.Command = defaults.Command
	}
	if options.Exit == nil {
		options.Exit = defaults.Exit
	}
	if options.Ready == nil {
		options.Ready = defaults.Ready
	}
	return options
}

func (client *Client) spawnOptions(options SpawnOptions) (SpawnOptions, error) {
	if err := validateTimeouts(options.Timeouts); err != nil {
		return options, err
	}
	if options.Backend == "" {
		options.Backend = client.options.Backend
	}
	if options.Profile == nil {
		options.Profile = client.options.Profile
	}
	options.Timeouts = withTimeoutDefaults(options.Timeouts, client.options.Timeouts)
	if options.Cols == nil {
		options.Cols = Ptr(uint16(80))
	}
	if options.Rows == nil {
		options.Rows = Ptr(uint16(30))
	}
	return options, nil
}
func (client *Client) spawn(options SpawnOptions, action func(SpawnOptions) (OpenResult, error)) (OpenResult, error) {
	resolved, err := client.spawnOptions(options)
	if err != nil {
		return OpenResult{}, err
	}
	for attempt := uint32(0); ; attempt++ {
		result, err := action(resolved)
		if err == nil || attempt == options.Retries {
			return result, err
		}
	}
}
func (client *Client) Open(options OpenOptions) (OpenResult, error) {
	return client.spawn(options.SpawnOptions, func(spawn SpawnOptions) (OpenResult, error) {
		return client.runtime.open(OpenOptions{spawn, options.Shell})
	})
}
func (client *Client) Run(program string, args []string, options SpawnOptions) (OpenResult, error) {
	return client.spawn(options, func(spawn SpawnOptions) (OpenResult, error) { return client.runtime.run(program, args, spawn) })
}
func (client *Client) Close() error { return client.runtime.close() }

// CloseQuiet attempts cleanup and reports success without returning diagnostics.
func (client *Client) CloseQuiet() bool               { return client.Close() == nil }
func (client *Client) Type(text string) error         { return client.runtime.typeText(text) }
func (client *Client) Write(data string) error        { return client.runtime.write(data) }
func (client *Client) Submit(text *string) error      { return client.runtime.submit(text) }
func (client *Client) Press(keys ...string) error     { return client.Keyboard.Press(keys...) }
func (client *Client) Resize(cols, rows uint16) error { return client.runtime.resize(cols, rows) }
func (client *Client) Signal(name string) error       { return client.runtime.signal(name) }
func (client *Client) Kill() error                    { return client.Signal("KILL") }
func (client *Client) State() (State, error)          { return client.runtime.state() }
func (client *Client) Text(options TextOptions) (string, error) {
	return client.runtime.text(options.Full)
}
func (client *Client) Cells(x, y, width, height uint16) ([]Cell, error) {
	return client.runtime.cells(x, y, width, height)
}
func (client *Client) GetCommand() (*string, error)        { return client.runtime.getCommand() }
func (client *Client) GetOutput() (*string, error)         { return client.runtime.getOutput() }
func (client *Client) GetExitCode() (*int32, error)        { return client.runtime.getExitCode() }
func (client *Client) GetCwd() (*string, error)            { return client.runtime.getCwd() }
func (client *Client) GetTitle() (*string, error)          { return client.runtime.getTitle() }
func (client *Client) GetClipboard() (string, error)       { return client.runtime.getClipboard() }
func (client *Client) GetCursor() (Cursor, error)          { return client.runtime.getCursor() }
func (client *Client) GetSize() (Size, error)              { return client.runtime.getSize() }
func (client *Client) GetBellCount() (uint64, error)       { return client.runtime.getBellCount() }
func (client *Client) GetBellEvents() ([]BellEvent, error) { return client.runtime.getBellEvents() }

// Screenshot returns terminal text when path is empty, otherwise the written path.
func (client *Client) Screenshot(path string, options ScreenshotOptions) (string, error) {
	if path == "" && options.Zoom != nil {
		return "", &Error{Kind: UsageError, Message: "screenshot zoom requires a path"}
	}
	return client.runtime.screenshot(path, options)
}
func (client *Client) StartRecording(path string, options RecordingOptions) error {
	return client.runtime.startRecording(path, options)
}
func (client *Client) StopRecording() (string, error) { return client.runtime.stopRecording() }

func (client *Client) guard(operation string, err error) error {
	var failure *Error
	if !errors.As(err, &failure) || failure.Kind != AssertionError {
		return err
	}
	copied := *failure
	copied.Operation = operation
	copied.Terminal = client.captureArtifact(failure.Message)
	return &copied
}
func (client *Client) captureArtifact(message string) *TerminalArtifact {
	options := client.options.Artifacts
	if options == nil || options.OnFailure == ArtifactNone {
		return nil
	}
	artifact := &TerminalArtifact{}
	if _, terminal, found := strings.Cut(message, "Terminal content:\n"); found {
		artifact.Text = strings.TrimRight(terminal, "\n")
	}
	if options.OnFailure == ArtifactText {
		return artifact
	}
	if err := os.MkdirAll(options.Dir, 0755); err != nil {
		artifact.CaptureError = err
		return artifact
	}
	path := filepath.Join(options.Dir, fmt.Sprintf("tuitest-%d-%d.svg", time.Now().UnixNano(), client.artifactCounter.Add(1)))
	artifact.Screenshot, artifact.CaptureError = client.Screenshot(path, ScreenshotOptions{})
	return artifact
}
func (client *Client) wait(operation string, timeout, fallback *time.Duration, action func(*time.Duration) error) error {
	resolved := timeout
	if resolved == nil {
		resolved = fallback
	}
	if resolved != nil {
		if _, err := timeoutMilliseconds(*resolved); err != nil {
			return err
		}
	}
	return client.guard(operation, action(resolved))
}
func (client *Client) WaitIdle(options WaitOptions) error {
	return client.wait("waitIdle", options.Timeout, client.options.Timeouts.Idle, client.runtime.waitIdle)
}
func (client *Client) WaitCommand(options WaitOptions) error {
	return client.wait("waitCommand", options.Timeout, client.options.Timeouts.Command, client.runtime.waitCommand)
}
func (client *Client) WaitExit(options WaitOptions) error {
	return client.wait("waitExit", options.Timeout, client.options.Timeouts.Exit, client.runtime.waitExit)
}
func (client *Client) WaitReady(options WaitOptions) error {
	return client.wait("waitReady", options.Timeout, client.options.Timeouts.Ready, client.runtime.waitReady)
}
func (client *Client) WaitBell(options WaitOptions) error {
	return client.wait("waitBell", options.Timeout, client.options.Timeouts.Text, client.runtime.waitBell)
}
func (client *Client) WaitTitle(text string, options TitleOptions) error {
	return client.wait("waitTitle", options.Timeout, client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return client.runtime.waitTitle(text, options)
	})
}
func (client *Client) WaitClipboard(options ClipboardWaitOptions) error {
	return client.wait("waitClipboard", options.Timeout, client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return client.runtime.waitClipboard(options)
	})
}
func (client *Client) ExpectTitle(text string, options TitleOptions) error {
	return client.wait("expectTitle", options.Timeout, client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return client.runtime.expectTitle(text, options)
	})
}
func (client *Client) ExpectExitCode(code int32, options WaitOptions) error {
	return client.wait("expectExitCode", options.Timeout, client.options.Timeouts.Command, func(timeout *time.Duration) error { return client.runtime.expectExitCode(code, timeout) })
}
func (client *Client) ExpectOutput(text string, options OutputOptions) error {
	return client.guard("expectOutput", client.runtime.expectOutput(text, options.Regex))
}
func (client *Client) ExpectBellCount(count uint64, options WaitOptions) error {
	return client.wait("expectBellCount", options.Timeout, client.options.Timeouts.Text, func(timeout *time.Duration) error { return client.runtime.expectBellCount(count, timeout) })
}
func (client *Client) ExpectSnapshot(name string, options SnapshotOptions) (SnapshotResult, error) {
	if options.Cwd == "" {
		directory, err := os.Getwd()
		if err != nil {
			return "", fmt.Errorf("snapshot working directory: %w", err)
		}
		options.Cwd = directory
	}
	result, err := client.runtime.snapshot(name, options)
	return result, client.guard("expectSnapshot", err)
}
