package native

import (
	"time"
)

func nativeDuration(value *time.Duration) (abiOptionalU64, error) {
	if value == nil {
		return abiOptionalU64{}, nil
	}
	milliseconds, err := Milliseconds(*value)
	if err != nil {
		return abiOptionalU64{}, err
	}
	return nativeUint(&milliseconds), nil
}

func nativeTimeouts(timeouts Timeouts) (abiTimeouts, error) {
	result := abiTimeouts{}
	targets := []*abiOptionalU64{&result.text, &result.idle, &result.command, &result.exit, &result.ready}
	for index, value := range []*time.Duration{timeouts.Text, timeouts.Idle, timeouts.Command, timeouts.Exit, timeouts.Ready} {
		converted, err := nativeDuration(value)
		if err != nil {
			return result, err
		}
		*targets[index] = converted
	}
	return result, nil
}

func (memory *nativeMemory) colors(colors Colors) (*abiPair, uintptr) {
	values := map[string]string{
		"foreground": colors.Foreground, "background": colors.Background, "cursor": colors.Cursor,
		"black": colors.Black, "red": colors.Red, "green": colors.Green, "yellow": colors.Yellow, "blue": colors.Blue, "magenta": colors.Magenta, "cyan": colors.Cyan, "white": colors.White,
		"bright_black": colors.BrightBlack, "bright_red": colors.BrightRed, "bright_green": colors.BrightGreen, "bright_yellow": colors.BrightYellow, "bright_blue": colors.BrightBlue, "bright_magenta": colors.BrightMagenta, "bright_cyan": colors.BrightCyan, "bright_white": colors.BrightWhite,
	}
	for key, value := range values {
		if value == "" {
			delete(values, key)
		}
	}
	return memory.pairs(values)
}

func (memory *nativeMemory) openOptions(options OpenOptions, recording *AutomaticRecording) (abiOpenOptions, error) {
	timeouts, err := nativeTimeouts(options.Timeouts)
	if err != nil {
		return abiOpenOptions{}, err
	}
	result := abiOpenOptions{backend: memory.nonemptyText(string(options.Backend)), shell: memory.nonemptyText(string(options.Shell)), cols: nativeUint(options.Cols), rows: nativeUint(options.Rows), cwd: memory.nonemptyText(options.Cwd), waitReady: nativeBool(options.WaitReady), timeouts: timeouts}
	if options.Restart != nil {
		result.restart = *options.Restart
	}
	result.env, result.envLen = memory.pairs(options.Env)
	if options.Profile != nil {
		result.scrollback = nativeUint(options.Profile.Scrollback)
		result.colors, result.colorsLen = memory.colors(options.Profile.Colors)
	}
	if recording != nil {
		result.recordingMode = memory.nonemptyText(string(recording.Mode))
		result.recordingDirectory = memory.nonemptyText(recording.Directory)
	}
	return result, nil
}

func nativeBool(value *bool) abiOptionalBool {
	if value == nil {
		return abiOptionalBool{}
	}
	return abiOptionalBool{present: true, value: *value}
}

func nativeUint[Number ~uint16 | ~uint32 | ~uint64](value *Number) abiOptionalU64 {
	if value == nil {
		return abiOptionalU64{}
	}
	return abiOptionalU64{present: true, value: uint64(*value)}
}

func nativeFloat(value *float64) abiOptionalF64 {
	if value == nil {
		return abiOptionalF64{}
	}
	return abiOptionalF64{present: true, value: float64(*value)}
}

func (memory *nativeMemory) style(style TextStyle) abiTextStyle {
	result := abiTextStyle{foreground: memory.optionalText(style.Foreground), background: memory.optionalText(style.Background), bold: nativeBool(style.Bold), dim: nativeBool(style.Dim), italic: nativeBool(style.Italic), underlineColor: memory.optionalText(style.UnderlineColor), inverse: nativeBool(style.Inverse), hidden: nativeBool(style.Hidden), strikethrough: nativeBool(style.Strikethrough), blink: nativeBool(style.Blink)}
	if style.UnderlineStyle != nil {
		result.underlineStyle = memory.text(string(*style.UnderlineStyle))
	}
	return result
}

func nativeDirection(direction Direction) uint32 {
	switch direction {
	case "", Within:
		return 0
	case After:
		return 1
	case Before:
		return 2
	default:
		return 3
	}
}

func nativeWhitespace(whitespace Whitespace) uint32 {
	switch whitespace {
	case "", Exact:
		return 0
	case Normalize:
		return 1
	default:
		return 2
	}
}

func nativeOccurrence(occurrence string) uint32 {
	switch occurrence {
	case "any":
		return 0
	case "unique":
		return 1
	case "first":
		return 2
	case "last":
		return 3
	case "nth":
		return 4
	default:
		return 5
	}
}

func (memory *nativeMemory) stage(stage LocatorStage) abiLocatorStage {
	converted := abiLocatorStage{style: memory.style(stage.Style), index: uintptr(stage.Nth), occurrence: nativeOccurrence(stage.Occurrence)}
	if stage.Kind == "style" {
		converted.kind = 1
		converted.full = stage.StyleOptions.Full
		converted.direction = nativeDirection(stage.StyleOptions.Direction)
		return converted
	}
	converted.text = memory.text(stage.Text)
	converted.regex = stage.TextOptions.Regex
	converted.full = stage.TextOptions.Full
	converted.whitespace = nativeWhitespace(stage.TextOptions.Whitespace)
	converted.direction = nativeDirection(stage.TextOptions.Direction)
	return converted
}

func (memory *nativeMemory) query(stages []LocatorStage) (abiQuery, error) {
	if len(stages) == 0 {
		return abiQuery{}, &Error{Kind: UsageError, Message: "locator requires at least one stage"}
	}
	entries := make([]abiLocatorStage, len(stages))
	pointer := &entries[0]
	memory.pinner.Pin(pointer)
	for index, stage := range stages {
		entries[index] = memory.stage(stage)
	}
	return abiQuery{stages: pointer, len: uintptr(len(stages))}, nil
}

func nativeMouse(options MouseButtonOptions) (abiMouseOptions, error) {
	result := abiMouseOptions{alt: options.Alt, ctrl: options.Ctrl, shift: options.Shift}
	switch options.Button {
	case "", Left:
	case Middle:
		result.button = 1
	case Right:
		result.button = 2
	default:
		return result, &Error{Kind: UsageError, Message: "unknown mouse button"}
	}
	return result, nil
}

func nativeClicks(clicks *uint32) (uint8, error) {
	if clicks == nil {
		return 1, nil
	}
	if *clicks > 255 {
		return 0, &Error{Kind: UsageError, Message: "clicks must be between 0 and 255"}
	}
	return uint8(*clicks), nil
}
