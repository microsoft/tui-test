package native

import (
	"encoding/json"
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
	result.style = memory.style(options.Style)
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

func nativePadding(value Padding) abiPadding {
	return abiPadding{uniform: nativeUint(value.Uniform), sides: abiPaddingSides{
		top: nativeUint(value.Top), right: nativeUint(value.Right),
		bottom: nativeUint(value.Bottom), left: nativeUint(value.Left),
	}}
}

func (memory *nativeMemory) style(value *RenderStyle) abiStyle {
	if value == nil {
		return abiStyle{}
	}
	files, length := memory.strings(value.Font.Files)
	return abiStyle{
		font: abiFontFamilies{family: memory.optionalText(value.Font.Family),
			bold: memory.optionalText(value.Font.Bold), italic: memory.optionalText(value.Font.Italic),
			boldItalic: memory.optionalText(value.Font.BoldItalic), files: files, filesLen: length},
		fontSize: nativeFloat(value.FontSize), titleFontSize: nativeFloat(value.TitleFontSize),
		canvasBackground: memory.optionalText(value.CanvasBackground),
		canvasPadding:    nativePadding(value.CanvasPadding), contentPadding: nativePadding(value.ContentPadding),
		window: abiWindowStyle{titleBar: nativeBool(value.Window.TitleBar), trafficLights: nativeBool(value.Window.TrafficLights),
			background: memory.optionalText(value.Window.Background), foreground: memory.optionalText(value.Window.Foreground), divider: memory.optionalText(value.Window.Divider)},
		border: abiBorderStyle{width: nativeFloat(value.Border.Width), radius: nativeFloat(value.Border.Radius), color: memory.optionalText(value.Border.Color)},
		shadow: abiShadowStyle{enabled: nativeBool(value.Shadow.Enabled), color: memory.optionalText(value.Shadow.Color), offset: nativeFloat(value.Shadow.Offset), spread: nativeFloat(value.Shadow.Spread)},
		cursor: abiCursorStyle{blink: nativeBool(value.Cursor.Blink), blinkIntervalMS: nativeUint(value.Cursor.BlinkIntervalMS)},
	}
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

func (memory *nativeMemory) query(expression LocatorExpression) (abiQuery, error) {
	if len(expression.Nodes) == 0 {
		return abiQuery{}, &Error{Kind: UsageError, Message: "locator requires at least one stage"}
	}
	value, err := json.Marshal(expression)
	if err != nil {
		return abiQuery{}, &Error{Kind: InternalError, Message: "encode locator expression: " + err.Error()}
	}
	return abiQuery{json: memory.text(string(value))}, nil
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
