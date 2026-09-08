package native

import "time"

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

type ScrollDirection string

const (
	Up   ScrollDirection = "up"
	Down ScrollDirection = "down"
)

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
type ArtifactOptions struct {
	Directory        string
	Mode             string
	IncludeRecording bool
}
type TraceOptions struct {
	Mode, Directory string
}

type SpawnOptions struct {
	Backend            Backend
	Cols, Rows         *uint16
	Cwd                string
	Env                map[string]string
	WaitReady, Restart *bool
	Profile            *Profile
	Timeouts           Timeouts
}
type SessionOptions struct {
	Recording          *AutomaticRecording
	Artifacts          *ArtifactOptions
	Trace              *TraceOptions
	ScreenHistoryLimit *uint16
}

type OpenOptions struct {
	SpawnOptions
	Shell Shell
}

type TitleOptions struct {
	Regex, Not bool
	Timeout    *time.Duration
}

type ClipboardWaitOptions struct {
	Text    *string
	Regex   bool
	Timeout *time.Duration
}

type ScreenshotOptions struct {
	Full        bool
	Zoom        *float64
	Background  string
	Transparent bool
}

type RecordingOptions struct {
	Format                     RecordingFormat
	FPS                        *uint32
	Speed, IdleTimeLimit, Zoom *float64
	Background                 string
	Transparent                bool
}

type SnapshotOptions struct {
	Update, IncludeStyle, IncludeTitle bool
	Cwd                                string
}

type SnapshotResult string

const (
	SnapshotPassed  SnapshotResult = "passed"
	SnapshotWritten SnapshotResult = "written"
	SnapshotUpdated SnapshotResult = "updated"
)

type TextStyle struct {
	Foreground     *string         `json:"foreground,omitempty"`
	Background     *string         `json:"background,omitempty"`
	Bold           *bool           `json:"bold,omitempty"`
	Dim            *bool           `json:"dim,omitempty"`
	Italic         *bool           `json:"italic,omitempty"`
	UnderlineStyle *UnderlineStyle `json:"underline_style,omitempty"`
	UnderlineColor *string         `json:"underline_color,omitempty"`
	Inverse        *bool           `json:"inverse,omitempty"`
	Hidden         *bool           `json:"hidden,omitempty"`
	Strikethrough  *bool           `json:"strikethrough,omitempty"`
	Blink          *bool           `json:"blink,omitempty"`
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

type Cursor struct {
	X, Y    uint16
	Visible bool
	Shape   string
	Color   string
}
type TerminalColors struct {
	Foreground, Background, Cursor string
	Palette                        map[uint8]string
}

type Size struct{ Cols, Rows uint16 }

type Color string

type Cell struct {
	X, Y                                                            uint16
	Char                                                            string
	FG, BG                                                          Color
	Bold, Dim, Italic, Inverse, Invisible, Strike, Blink, Underline bool
	UnderlineStyle                                                  UnderlineStyle
	UnderlineColor                                                  Color
	Link, LinkID                                                    string
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
	LastExit                *int32
	ExitSignal              *string
	Exited                  *int32
	Ready                   bool
	BellCount               uint64
	Modes                   map[string]bool
	MouseMode               string
	Colors                  TerminalColors
	Timeouts                EffectiveTimeouts
	Text                    string
}

type ErrorKind string

const (
	AssertionError ErrorKind = "assertion"
	UsageError     ErrorKind = "usage"
	NoSessionError ErrorKind = "no_session"
	InternalError  ErrorKind = "internal"
)

type Error struct {
	Kind         ErrorKind
	Message      string
	DetailsJSON  []byte
	ArtifactJSON []byte
}

func (failure *Error) Error() string          { return failure.Message }
func pointerTo[Value any](value Value) *Value { return &value }

type LocatorNode struct {
	Kind       string      `json:"kind"`
	Text       *string     `json:"text,omitempty"`
	Regex      *bool       `json:"regex,omitempty"`
	Whitespace *Whitespace `json:"whitespace,omitempty"`
	Style      *TextStyle  `json:"style,omitempty"`
	Link       *string     `json:"link,omitempty"`
	Full       *bool       `json:"full,omitempty"`
	Direction  *Direction  `json:"direction,omitempty"`
	Within     *uint32     `json:"within,omitempty"`
	Left       *uint32     `json:"left,omitempty"`
	Right      *uint32     `json:"right,omitempty"`
	Input      *uint32     `json:"input,omitempty"`
	Has        *uint32     `json:"has,omitempty"`
	HasNot     *uint32     `json:"has_not,omitempty"`
	Occurrence any         `json:"occurrence"`
}
type LocatorExpression struct {
	Nodes []LocatorNode `json:"nodes"`
	Root  uint32        `json:"root"`
}

func Milliseconds(timeout time.Duration) (uint64, error) {
	if timeout < 0 {
		return 0, &Error{Kind: UsageError, Message: "timeout must not be negative"}
	}
	milliseconds := uint64(timeout / time.Millisecond)
	if timeout%time.Millisecond != 0 {
		milliseconds++
	}
	return milliseconds, nil
}
