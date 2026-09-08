package tuitest

// Keyboard sends terminal key events, including separate press and release events.
type Keyboard struct{ runtime *nativeRuntime }

func (keyboard *Keyboard) Press(keys ...string) error  { return keyboard.runtime.key(keys, 0) }
func (keyboard *Keyboard) Down(keys ...string) error   { return keyboard.runtime.key(keys, 1) }
func (keyboard *Keyboard) Repeat(keys ...string) error { return keyboard.runtime.key(keys, 2) }
func (keyboard *Keyboard) Up(keys ...string) error     { return keyboard.runtime.key(keys, 3) }

// Mouse sends coordinates in terminal cells.
type Mouse struct{ runtime *nativeRuntime }

func (mouse *Mouse) Click(options MouseClickOptions) error { return mouse.runtime.mouseClick(options) }
func (mouse *Mouse) Move(x, y uint16) error                { return mouse.runtime.mouseMove(x, y) }
func (mouse *Mouse) Down(x, y uint16, options MouseButtonOptions) error {
	return mouse.runtime.mouseDown(x, y, options)
}
func (mouse *Mouse) Up(x, y uint16, options MouseButtonOptions) error {
	return mouse.runtime.mouseUp(x, y, options)
}
func (mouse *Mouse) Drag(x1, y1, x2, y2 uint16, options MouseButtonOptions) error {
	return mouse.runtime.mouseDrag(x1, y1, x2, y2, options)
}
func (mouse *Mouse) Scroll(direction ScrollDirection, amount uint32) error {
	return mouse.runtime.mouseScroll(direction, amount)
}
