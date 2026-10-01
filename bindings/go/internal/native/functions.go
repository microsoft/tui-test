package native

import "slices"

type nativeFunctionTable struct {
	AbiVersion       func() uint32
	ResultFree       func(*abiResult)
	Open             func(abiString, *abiOpenOptions) *abiResult
	Run              func(abiString, *abiOpenOptions, abiString, *abiString, uintptr) *abiResult
	Sessions         func() *abiResult
	CloseAll         func() *abiResult
	Recording        func(abiString) *abiResult
	Close            func(abiString) *abiResult
	State            func(abiString) *abiResult
	GetCommand       func(abiString) *abiResult
	GetOutput        func(abiString) *abiResult
	GetExitCode      func(abiString) *abiResult
	GetCwd           func(abiString) *abiResult
	GetCursor        func(abiString) *abiResult
	GetSize          func(abiString) *abiResult
	GetTitle         func(abiString) *abiResult
	GetClipboard     func(abiString) *abiResult
	GetBellCount     func(abiString) *abiResult
	GetBellEvents    func(abiString) *abiResult
	StopRecording    func(abiString) *abiResult
	Text             func(abiString, bool) *abiResult
	PackedScreen     func(abiString, bool) *abiResult
	Cells            func(abiString, uint16, uint16, uint16, uint16) *abiResult
	Write            func(abiString, abiString) *abiResult
	Submit           func(abiString, abiString) *abiResult
	Signal           func(abiString, abiString) *abiResult
	Key              func(abiString, *abiString, uintptr, uint32) *abiResult
	Resize           func(abiString, uint16, uint16) *abiResult
	MouseClick       func(abiString, abiOptionalU64, abiOptionalU64, abiString, abiMouseOptions, uint8) *abiResult
	MouseMove        func(abiString, uint16, uint16) *abiResult
	MouseDown        func(abiString, uint16, uint16, abiMouseOptions) *abiResult
	MouseUp          func(abiString, uint16, uint16, abiMouseOptions) *abiResult
	MouseDrag        func(abiString, uint16, uint16, uint16, uint16, abiMouseOptions) *abiResult
	MouseScroll      func(abiString, abiString, uint16) *abiResult
	WaitTitle        func(abiString, abiString, abiWaitOptions) *abiResult
	ExpectTitle      func(abiString, abiString, abiWaitOptions) *abiResult
	WaitClipboard    func(abiString, abiString, abiWaitOptions) *abiResult
	WaitIdle         func(abiString, abiOptionalU64) *abiResult
	WaitCommand      func(abiString, abiOptionalU64) *abiResult
	WaitExit         func(abiString, abiOptionalU64) *abiResult
	WaitReady        func(abiString, abiOptionalU64) *abiResult
	WaitBell         func(abiString, abiOptionalU64) *abiResult
	FindLocator      func(abiString, abiQuery) *abiResult
	WaitLocator      func(abiString, abiQuery, abiWaitOptions) *abiResult
	ClickLocator     func(abiString, abiQuery, abiMouseOptions, uint8, abiOptionalU64) *abiResult
	HighlightLocator func(abiString, abiQuery, abiOptionalU64) *abiResult
	ExpectExitCode   func(abiString, int32, abiOptionalU64) *abiResult
	ExpectOutput     func(abiString, abiString, bool) *abiResult
	ExpectBellCount  func(abiString, uint64, abiOptionalU64) *abiResult
	Snapshot         func(abiString, abiString, bool, bool, bool, abiString) *abiResult
	Screenshot       func(abiString, bool, abiString, abiOptionalF64) *abiResult
	StartRecording   func(abiString, abiRecordingOptions) *abiResult
}

func nativeRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return slices.Concat(nativeLifecycleRegistrations(table), nativeInputRegistrations(table), nativeInspectionRegistrations(table), nativeWaitingRegistrations(table), nativeArtifactsRegistrations(table))
}
func nativeLifecycleRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return []nativeRegistration{
		{symbol: "tui_abi_version", target: &table.AbiVersion},
		{symbol: "tui_result_free", target: &table.ResultFree},
		{symbol: "tui_open_ptr", target: &table.Open},
		{symbol: "tui_run_ptr", target: &table.Run},
		{symbol: "tui_sessions", target: &table.Sessions},
		{symbol: "tui_close_all", target: &table.CloseAll},
		{symbol: "tui_recording", target: &table.Recording},
		{symbol: "tui_close", target: &table.Close},
	}
}
func nativeInputRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return []nativeRegistration{
		{symbol: "tui_write", target: &table.Write},
		{symbol: "tui_submit", target: &table.Submit},
		{symbol: "tui_signal", target: &table.Signal},
		{symbol: "tui_key", target: &table.Key},
		{symbol: "tui_resize", target: &table.Resize},
		{symbol: "tui_mouse_click", target: &table.MouseClick},
		{symbol: "tui_mouse_move", target: &table.MouseMove},
		{symbol: "tui_mouse_down", target: &table.MouseDown},
		{symbol: "tui_mouse_up", target: &table.MouseUp},
		{symbol: "tui_mouse_drag", target: &table.MouseDrag},
		{symbol: "tui_mouse_scroll", target: &table.MouseScroll},
	}
}
func nativeInspectionRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return []nativeRegistration{
		{symbol: "tui_state", target: &table.State},
		{symbol: "tui_get_command", target: &table.GetCommand},
		{symbol: "tui_get_output", target: &table.GetOutput},
		{symbol: "tui_get_exit_code", target: &table.GetExitCode},
		{symbol: "tui_get_cwd", target: &table.GetCwd},
		{symbol: "tui_get_cursor", target: &table.GetCursor},
		{symbol: "tui_get_size", target: &table.GetSize},
		{symbol: "tui_get_title", target: &table.GetTitle},
		{symbol: "tui_get_clipboard", target: &table.GetClipboard},
		{symbol: "tui_get_bell_count", target: &table.GetBellCount},
		{symbol: "tui_get_bell_events", target: &table.GetBellEvents},
		{symbol: "tui_text", target: &table.Text},
		{symbol: "tui_packed_screen", target: &table.PackedScreen},
		{symbol: "tui_cells", target: &table.Cells},
	}
}
func nativeWaitingRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return []nativeRegistration{
		{symbol: "tui_wait_title", target: &table.WaitTitle},
		{symbol: "tui_expect_title", target: &table.ExpectTitle},
		{symbol: "tui_wait_clipboard", target: &table.WaitClipboard},
		{symbol: "tui_wait_idle", target: &table.WaitIdle},
		{symbol: "tui_wait_command", target: &table.WaitCommand},
		{symbol: "tui_wait_exit", target: &table.WaitExit},
		{symbol: "tui_wait_ready", target: &table.WaitReady},
		{symbol: "tui_wait_bell", target: &table.WaitBell},
		{symbol: "tui_find_locator", target: &table.FindLocator},
		{symbol: "tui_wait_locator", target: &table.WaitLocator},
		{symbol: "tui_click_locator", target: &table.ClickLocator},
		{symbol: "tui_highlight_locator", target: &table.HighlightLocator},
		{symbol: "tui_expect_exit_code", target: &table.ExpectExitCode},
		{symbol: "tui_expect_output", target: &table.ExpectOutput},
		{symbol: "tui_expect_bell_count", target: &table.ExpectBellCount},
	}
}
func nativeArtifactsRegistrations(table *nativeFunctionTable) []nativeRegistration {
	return []nativeRegistration{
		{symbol: "tui_stop_recording", target: &table.StopRecording},
		{symbol: "tui_snapshot", target: &table.Snapshot},
		{symbol: "tui_screenshot", target: &table.Screenshot},
		{symbol: "tui_start_recording", target: &table.StartRecording},
	}
}
