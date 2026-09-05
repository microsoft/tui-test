package native

import (
	"structs"
	"unsafe"
)

// These host-layout structures mirror the private C ABI in native.h.
type abiString struct {
	_    structs.HostLayout
	data *uint8
	len  uintptr
}
type abiOptionalI32 struct {
	_       structs.HostLayout
	present bool
	value   int32
}
type abiOptionalU64 struct {
	_       structs.HostLayout
	present bool
	value   uint64
}
type abiOpenResult struct {
	_         structs.HostLayout
	shellPID  abiOptionalU64
	session   abiString
	ready     bool
	recording abiString
}
type abiCursor struct {
	_ structs.HostLayout
	x uint16
	y uint16
}
type abiTimeouts struct {
	_       structs.HostLayout
	text    abiOptionalU64
	idle    abiOptionalU64
	command abiOptionalU64
	exit    abiOptionalU64
	ready   abiOptionalU64
}
type abiState struct {
	_            structs.HostLayout
	sessionShell abiString
	cols         uint16
	rows         uint16
	cursor       abiCursor
	title        abiString
	cwd          abiString
	lastCommand  abiString
	lastExit     abiOptionalI32
	exited       abiOptionalI32
	ready        bool
	bellCount    uint64
	timeouts     abiTimeouts
	text         abiString
}
type abiSize struct {
	_    structs.HostLayout
	cols uint16
	rows uint16
}
type abiColor struct {
	_     structs.HostLayout
	kind  uint32
	index uint8
	red   uint8
	green uint8
	blue  uint8
}
type abiCell struct {
	_              structs.HostLayout
	x              uint16
	y              uint16
	character      abiString
	fg             abiColor
	bg             abiColor
	bold           bool
	dim            bool
	italic         bool
	inverse        bool
	invisible      bool
	strike         bool
	blink          bool
	underline      bool
	underlineStyle abiString
	underlineColor abiColor
}
type abiPosition struct {
	_      structs.HostLayout
	row    uint32
	column uint16
}
type abiSpan struct {
	_     structs.HostLayout
	row   uint32
	start uint16
	end   uint16
}
type abiMatch struct {
	_        structs.HostLayout
	text     abiString
	start    abiPosition
	end      abiPosition
	spans    *abiSpan
	spansLen uintptr
}
type abiBellEvent struct {
	_         structs.HostLayout
	sequence  uint64
	elapsedMS uint64
}
type abiResult struct {
	_            structs.HostLayout
	errorKind    uint32
	errorMessage abiString
	text         abiString
	number       uint64
	exitCode     abiOptionalI32
	open         abiOpenResult
	state        abiState
	cursor       abiCursor
	size         abiSize
	cells        *abiCell
	cellsLen     uintptr
	matches      *abiMatch
	matchesLen   uintptr
	bells        *abiBellEvent
	bellsLen     uintptr
	strings      *abiString
	stringsLen   uintptr
	snapshot     uint32
	privateData  unsafe.Pointer
}
type abiPair struct {
	_     structs.HostLayout
	key   abiString
	value abiString
}
type abiOptionalBool struct {
	_       structs.HostLayout
	present bool
	value   bool
}
type abiOpenOptions struct {
	_                  structs.HostLayout
	backend            abiString
	shell              abiString
	cols               abiOptionalU64
	rows               abiOptionalU64
	cwd                abiString
	env                *abiPair
	envLen             uintptr
	waitReady          abiOptionalBool
	restart            bool
	scrollback         abiOptionalU64
	colors             *abiPair
	colorsLen          uintptr
	timeouts           abiTimeouts
	recordingMode      abiString
	recordingDirectory abiString
}
type abiMouseOptions struct {
	_      structs.HostLayout
	button uint32
	alt    bool
	ctrl   bool
	shift  bool
}
type abiWaitOptions struct {
	_         structs.HostLayout
	timeoutMS abiOptionalU64
	regex     bool
	not       bool
}
type abiAnchor struct {
	_          structs.HostLayout
	text       abiString
	regex      bool
	occurrence uint32
	index      uintptr
}
type abiTextStyle struct {
	_              structs.HostLayout
	foreground     abiString
	background     abiString
	bold           abiOptionalBool
	dim            abiOptionalBool
	italic         abiOptionalBool
	underlineStyle abiString
	underlineColor abiString
	inverse        abiOptionalBool
	hidden         abiOptionalBool
	strikethrough  abiOptionalBool
	blink          abiOptionalBool
}
type abiLocatorStage struct {
	_          structs.HostLayout
	kind       uint32
	text       abiString
	regex      bool
	full       bool
	whitespace uint32
	after      abiAnchor
	before     abiAnchor
	style      abiTextStyle
	occurrence uint32
	index      uintptr
	direction  uint32
}
type abiQuery struct {
	_      structs.HostLayout
	stages *abiLocatorStage
	len    uintptr
}
type abiOptionalF64 struct {
	_       structs.HostLayout
	present bool
	value   float64
}
type abiRecordingOptions struct {
	_             structs.HostLayout
	path          abiString
	format        abiString
	fps           abiOptionalU64
	speed         abiOptionalF64
	idleTimeLimit abiOptionalF64
	zoom          abiOptionalF64
}
