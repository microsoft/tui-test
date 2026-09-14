package native

import (
	"fmt"
	"math"
	"strconv"
	"time"
	"unsafe"
)

func nativeText(value abiString) string {
	if value.data == nil {
		return ""
	}
	return string(unsafe.Slice(value.data, value.len)) //nolint:gosec // G103: Copies Rust-owned bytes into a Go string before the owning result is freed.
}

func nativeOptionalText(value abiString) *string {
	if value.data == nil {
		return nil
	}
	return pointerTo(nativeText(value))
}

func readOpenResult(result *abiResult) (OpenResult, error) {
	if err := nativeError(result); err != nil {
		return OpenResult{}, err
	}
	opened := result.open
	converted := OpenResult{Session: nativeText(opened.session), Ready: opened.ready, Recording: nativeText(opened.recording)}
	if opened.shellPID.present {
		if opened.shellPID.value > math.MaxUint32 {
			return OpenResult{}, &Error{Kind: InternalError, Message: "native process identifier exceeds uint32"}
		}
		converted.ShellPID = pointerTo(uint32(opened.shellPID.value))
	}
	return converted, nil
}

func nativeInt(value abiOptionalI32) *int32 {
	if !value.present {
		return nil
	}
	return pointerTo(value.value)
}

func nativeError(result *abiResult) error {
	if result == nil {
		return &Error{Kind: InternalError, Message: "native engine returned no result"}
	}
	if result.errorKind == 0 {
		return nil
	}
	kind := InternalError
	switch result.errorKind {
	case 1:
		kind = AssertionError
	case 2:
		kind = UsageError
	case 3:
		kind = NoSessionError
	}
	return &Error{Kind: kind, Message: nativeText(result.errorMessage)}
}

func nativeColor(value abiColor) Color {
	switch value.kind {
	case 1:
		return Color(strconv.Itoa(int(value.index)))
	case 2:
		return Color(fmt.Sprintf("#%02x%02x%02x", value.red, value.green, value.blue))
	default:
		return "default"
	}
}

func nativeCells(result *abiResult) []Cell {
	cells := make([]Cell, int(result.cellsLen))
	for index, cell := range unsafe.Slice(result.cells, int(result.cellsLen)) { //nolint:gosec // G103: Rust supplies the cell buffer and length; all fields are copied before ResultFree.
		cells[index] = Cell{X: cell.x, Y: cell.y, Char: nativeText(cell.character), FG: nativeColor(cell.fg), BG: nativeColor(cell.bg), Bold: cell.bold, Dim: cell.dim, Italic: cell.italic, Inverse: cell.inverse, Invisible: cell.invisible, Strike: cell.strike, Blink: cell.blink, Underline: cell.underline, UnderlineStyle: UnderlineStyle(nativeText(cell.underlineStyle)), UnderlineColor: nativeColor(cell.underlineColor)}
	}
	return cells
}

func nativeMatches(result *abiResult) []TextMatch {
	matches := make([]TextMatch, int(result.matchesLen))
	for index, match := range unsafe.Slice(result.matches, int(result.matchesLen)) { //nolint:gosec // G103: Rust supplies the match buffer and length; nested data is copied before ResultFree.
		spans := make([]TextSpan, int(match.spansLen))
		for spanIndex, span := range unsafe.Slice(match.spans, int(match.spansLen)) { //nolint:gosec // G103: The owning Rust result keeps spans alive until this copy finishes, before ResultFree.
			spans[spanIndex] = TextSpan{Row: span.row, Start: uint32(span.start), End: uint32(span.end)}
		}
		matches[index] = TextMatch{Text: nativeText(match.text), Start: TextPosition{Row: match.start.row, Column: uint32(match.start.column)}, End: TextPosition{Row: match.end.row, Column: uint32(match.end.column)}, Spans: spans}
	}
	return matches
}

func nativeState(value abiState) State {
	return State{SessionShell: nativeOptionalText(value.sessionShell), Cols: value.cols, Rows: value.rows, Cursor: Cursor{X: value.cursor.x, Y: value.cursor.y}, Title: nativeOptionalText(value.title), Cwd: nativeOptionalText(value.cwd), LastCommand: nativeOptionalText(value.lastCommand), LastExit: nativeInt(value.lastExit), Exited: nativeInt(value.exited), Ready: value.ready, BellCount: value.bellCount, Text: nativeText(value.text), Timeouts: EffectiveTimeouts{Text: nativeMilliseconds(value.timeouts.text.value), Idle: nativeMilliseconds(value.timeouts.idle.value), Command: nativeMilliseconds(value.timeouts.command.value), Exit: nativeMilliseconds(value.timeouts.exit.value), Ready: nativeMilliseconds(value.timeouts.ready.value)}}
}

// Millisecond precision can round a maximum Go duration beyond its range.
func nativeMilliseconds(value uint64) time.Duration {
	if value > uint64(math.MaxInt64/int64(time.Millisecond)) {
		return time.Duration(math.MaxInt64)
	}
	return time.Duration(value) * time.Millisecond
}

func nativeSessionNames(result *abiResult) []string {
	sessions := make([]string, int(result.stringsLen))
	for index, value := range unsafe.Slice(result.strings, int(result.stringsLen)) { //nolint:gosec // G103: Rust supplies the session buffer and length; each string is copied before deferred ResultFree.
		sessions[index] = nativeText(value)
	}
	return sessions
}

func nativeBellEvents(result *abiResult) []BellEvent {
	events := make([]BellEvent, int(result.bellsLen))
	for index, value := range unsafe.Slice(result.bells, int(result.bellsLen)) { //nolint:gosec // G103: Rust supplies the event buffer and length; values are copied before deferred ResultFree.
		events[index] = BellEvent{Sequence: value.sequence, Elapsed: nativeMilliseconds(value.elapsedMS)}
	}
	return events
}
