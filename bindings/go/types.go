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
	ArtifactAll  ArtifactMode = "all"
	ArtifactHTML ArtifactMode = "html"
	ArtifactText ArtifactMode = "text"
	ArtifactNone ArtifactMode = "none"
)

type ArtifactOptions struct {
	Dir              string
	OnFailure        ArtifactMode
	IncludeRecording bool
}
type TraceMode string

const (
	TraceOff       TraceMode = "off"
	TraceOnFailure TraceMode = "on-failure"
	TraceOn        TraceMode = "on"
)

type TraceOptions struct {
	Mode      TraceMode
	Directory string
}
type ClientOptions struct {
	Backend            Backend
	Profile            *Profile
	Timeouts           Timeouts
	ScreenHistoryLimit *uint16
	Recording          *AutomaticRecording
	Artifacts          *ArtifactOptions
	Trace              *TraceOptions
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
type RestartOptions struct{ GracefulTimeout *time.Duration }
type CloseOptions struct{ Failed *bool }
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
type Cursor struct {
	X, Y    uint16
	Visible bool
	Shape   string
	Color   string
}
type Size struct{ Cols, Rows uint16 }
type TerminalColors struct {
	Foreground, Background, Cursor string
	Palette                        map[uint8]string
}

// Color is "default", a named or RGB color, or an indexed decimal color.
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

type FailureReason string
type LocatorFailureReason string
type FailureCellMismatch struct {
	Location TextPosition `json:"location"`
	Grapheme string       `json:"grapheme"`
	Property string       `json:"property"`
	Operator string       `json:"operator"`
	Expected string       `json:"expected"`
	Actual   string       `json:"actual"`
	Resolved *string      `json:"resolved,omitempty"`
	Reason   string       `json:"reason"`
}
type FailureLocatorDetails struct {
	Reason     *LocatorFailureReason `json:"reason,omitempty"`
	Selectors  []string              `json:"selectors"`
	StageIndex *uint32               `json:"stage_index,omitempty"`
	Locations  []TextPosition        `json:"locations"`
	Mismatches []FailureCellMismatch `json:"mismatches"`
}
type FailureComparison struct {
	Kind     string  `json:"kind"`
	Expected *string `json:"expected,omitempty"`
	Actual   *string `json:"actual,omitempty"`
}
type FailureDetails struct {
	SchemaVersion uint32                 `json:"schema_version"`
	Operation     string                 `json:"operation"`
	Reason        FailureReason          `json:"reason"`
	Summary       string                 `json:"summary"`
	Locator       *FailureLocatorDetails `json:"locator,omitempty"`
	Comparison    *FailureComparison     `json:"comparison,omitempty"`
	Truncated     bool                   `json:"truncated"`
}
type FailureArtifactStatus string
type FailureArtifactRef struct {
	Status     FailureArtifactStatus `json:"status"`
	Directory  string                `json:"directory"`
	Manifest   *string               `json:"manifest,omitempty"`
	Report     *string               `json:"report,omitempty"`
	ReportHTML *string               `json:"report_html,omitempty"`
	Timeline   *string               `json:"timeline,omitempty"`
	ScreenText *string               `json:"screen_text,omitempty"`
	ScreenSVG  *string               `json:"screen_svg,omitempty"`
	Recording  *string               `json:"recording,omitempty"`
	Errors     []string              `json:"errors,omitempty"`
}

// Error retains the engine's category and diagnostic message for errors.As.
type Error struct {
	Kind      ErrorKind
	Message   string
	Operation string
	Details   *FailureDetails
	Artifact  *FailureArtifactRef
}

func (failure *Error) Error() string {
	if failure.Operation != "" {
		return failure.Operation + ": " + failure.Message
	}
	return failure.Message
}
