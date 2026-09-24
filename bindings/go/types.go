// Package tuitest drives terminal programs through the in-process tui-test engine.
package tuitest

import "time"

// Ptr marks an option as explicitly supplied, including false and zero.
func Ptr[Value any](value Value) *Value { return &value }

type Backend string

const (
	Alacritty Backend = "alacritty"
	Ghostty   Backend = "ghostty"
	Rio       Backend = "rio"
	XtermJS   Backend = "xtermjs"
)

type Shell string

const (
	Bash       Shell = "bash"
	PowerShell Shell = "powershell"
	Pwsh       Shell = "pwsh"
	Cmd        Shell = "cmd"
	Fish       Shell = "fish"
	Zsh        Shell = "zsh"
	Xonsh      Shell = "xonsh"
	Elvish     Shell = "elvish"
	Nushell    Shell = "nushell"
)

type UnderlineStyle string

const (
	UnderlineNone   UnderlineStyle = "none"
	UnderlineSingle UnderlineStyle = "single"
	UnderlineDouble UnderlineStyle = "double"
	UnderlineCurly  UnderlineStyle = "curly"
	UnderlineDotted UnderlineStyle = "dotted"
	UnderlineDashed UnderlineStyle = "dashed"
)

type RecordingMode string

const (
	RecordingDisabled  RecordingMode = "disabled"
	RecordingOnFailure RecordingMode = "on-failure"
	RecordingAlways    RecordingMode = "always"
)

type RecordingFormat string

const (
	APNG RecordingFormat = "apng"
	GIF  RecordingFormat = "gif"
	MP4  RecordingFormat = "mp4"
	Cast RecordingFormat = "cast"
)

type MouseButton string

const (
	Left   MouseButton = "left"
	Middle MouseButton = "middle"
	Right  MouseButton = "right"
)

type Direction string

const (
	Within Direction = "within"
	After  Direction = "after"
	Before Direction = "before"
)

type Whitespace string

const (
	Exact     Whitespace = "exact"
	Normalize Whitespace = "normalize"
)

type Visibility string

const (
	Visible Visibility = "visible"
	Hidden  Visibility = "hidden"
)

type ScrollDirection string

const (
	Up   ScrollDirection = "up"
	Down ScrollDirection = "down"
)

// Timeouts leaves nil fields unspecified and preserves explicit zero durations.
type Timeouts struct{ Text, Idle, Command, Exit, Ready *time.Duration }
type EffectiveTimeouts struct{ Text, Idle, Command, Exit, Ready time.Duration }
type Colors struct {
	Foreground, Background, Cursor                                                                        string
	Black, Red, Green, Yellow, Blue, Magenta, Cyan, White                                                 string
	BrightBlack, BrightRed, BrightGreen, BrightYellow, BrightBlue, BrightMagenta, BrightCyan, BrightWhite string
}
type Profile struct {
	Scrollback *uint32
	Colors     Colors
}
type AutomaticRecording struct {
	Mode      RecordingMode
	Directory string
}
type ArtifactMode string

const (
	ArtifactSVG  ArtifactMode = "svg"
	ArtifactText ArtifactMode = "text"
	ArtifactNone ArtifactMode = "none"
)

type ArtifactOptions struct {
	Dir       string
	OnFailure ArtifactMode
}
type ClientOptions struct {
	Backend   Backend
	Profile   *Profile
	Timeouts  Timeouts
	Recording *AutomaticRecording
	Artifacts *ArtifactOptions
}
type SpawnOptions struct {
	Backend            Backend
	Cols, Rows         *uint16
	Cwd                string
	Env                map[string]string
	WaitReady, Restart *bool
	Retries            uint32
	Profile            *Profile
	Timeouts           Timeouts
}
type OpenOptions struct {
	SpawnOptions
	Shell Shell
}
type WaitOptions struct{ Timeout *time.Duration }
type TitleOptions struct {
	Regex, Not bool
	Timeout    *time.Duration
}
type ClipboardWaitOptions struct {
	Text    *string
	Regex   bool
	Timeout *time.Duration
}
type TextOptions struct{ Full bool }
type ScreenshotOptions struct {
	Full bool
	Zoom *float64
}
type RecordingOptions struct {
	Format                     RecordingFormat
	FPS                        *uint32
	Speed, IdleTimeLimit, Zoom *float64
}
type SnapshotOptions struct {
	Update, IncludeColors, IncludeTitle bool
	Cwd                                 string
}
type SnapshotResult string

const (
	SnapshotPassed  SnapshotResult = "passed"
	SnapshotWritten SnapshotResult = "written"
	SnapshotUpdated SnapshotResult = "updated"
)

type OutputOptions struct{ Regex bool }
type TextSelectorOptions struct {
	Regex, Full bool
	Whitespace  Whitespace
	Direction   Direction
}
type StyleSelectorOptions struct {
	Full      bool
	Direction Direction
}
type TextStyle struct {
	Foreground, Background                *string
	Bold, Dim, Italic                     *bool
	UnderlineStyle                        *UnderlineStyle
	UnderlineColor                        *string
	Inverse, Hidden, Strikethrough, Blink *bool
}
type LocatorWaitOptions struct {
	State   Visibility
	Timeout *time.Duration
}
type LocatorExpectOptions struct {
	Not     bool
	Timeout *time.Duration
}
type MouseButtonOptions struct {
	Button           MouseButton
	Alt, Ctrl, Shift bool
}
type MouseClickOptions struct {
	MouseButtonOptions
	X, Y   *uint16
	OnText *string
	Clicks *uint32
}
type LocatorClickOptions struct {
	MouseButtonOptions
	Clicks  *uint32
	Timeout *time.Duration
}
type Cursor struct{ X, Y uint16 }
type Size struct{ Cols, Rows uint16 }

// Color is "default", a named or RGB color, or an indexed decimal color.
type Color string
type Cell struct {
	X, Y                                                            uint16
	Char                                                            string
	FG, BG                                                          Color
	Bold, Dim, Italic, Inverse, Invisible, Strike, Blink, Underline bool
	UnderlineStyle                                                  UnderlineStyle
	UnderlineColor                                                  Color
}
type BellEvent struct {
	Sequence uint64
	Elapsed  time.Duration
}
type TextPosition struct{ Row, Column uint32 }
type TextSpan struct{ Row, Start, End uint32 }
type TextMatch struct {
	Text       string
	Start, End TextPosition
	Spans      []TextSpan
}
type OpenResult struct {
	ShellPID  *uint32
	Session   string
	Ready     bool
	Recording string
}
type State struct {
	SessionShell            *string
	Cols, Rows              uint16
	Cursor                  Cursor
	Title, Cwd, LastCommand *string
	LastExit, Exited        *int32
	Ready                   bool
	BellCount               uint64
	Timeouts                EffectiveTimeouts
	Text                    string
}
type ErrorKind string

const (
	AssertionError ErrorKind = "assertion"
	UsageError     ErrorKind = "usage"
	NoSessionError ErrorKind = "no-session"
	InternalError  ErrorKind = "internal"
)

type TerminalArtifact struct {
	Text, Screenshot string
	CaptureError     error
}

// Error retains the engine's category and diagnostic message for errors.As.
type Error struct {
	Kind      ErrorKind
	Message   string
	Operation string
	Terminal  *TerminalArtifact
}

func (failure *Error) Error() string {
	if failure.Operation != "" {
		return failure.Operation + ": " + failure.Message
	}
	return failure.Message
}
