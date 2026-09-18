package tuitest

import "time"

type locatorStage struct {
	kind         string
	text         string
	textOptions  TextSelectorOptions
	style        TextStyle
	styleOptions StyleSelectorOptions
	occurrence   string
	nth          uint32
}

// Locator is an immutable query, resolved against the current grid on each operation.
// Invalid selector options are reported when an operation resolves the locator.
type Locator struct {
	client *Client
	stages []locatorStage
}

func (client *Client) GetByText(text string, options TextSelectorOptions) *Locator {
	return (&Locator{client: client}).GetByText(text, options)
}
func (client *Client) GetByStyle(style TextStyle, options StyleSelectorOptions) *Locator {
	return (&Locator{client: client}).GetByStyle(style, options)
}
func (locator *Locator) append(stage locatorStage) *Locator {
	stages := make([]locatorStage, len(locator.stages)+1)
	copy(stages, locator.stages)
	stages[len(locator.stages)] = stage
	return &Locator{client: locator.client, stages: stages}
}
func (locator *Locator) GetByText(text string, options TextSelectorOptions) *Locator {
	return locator.append(locatorStage{kind: "text", text: text, textOptions: options, occurrence: "any"})
}
func (locator *Locator) GetByStyle(style TextStyle, options StyleSelectorOptions) *Locator {
	style.Foreground = clonePointer(style.Foreground)
	style.Background = clonePointer(style.Background)
	style.Bold = clonePointer(style.Bold)
	style.Dim = clonePointer(style.Dim)
	style.Italic = clonePointer(style.Italic)
	style.UnderlineStyle = clonePointer(style.UnderlineStyle)
	style.UnderlineColor = clonePointer(style.UnderlineColor)
	style.Inverse = clonePointer(style.Inverse)
	style.Hidden = clonePointer(style.Hidden)
	style.Strikethrough = clonePointer(style.Strikethrough)
	style.Blink = clonePointer(style.Blink)
	return locator.append(locatorStage{kind: "style", style: style, styleOptions: options, occurrence: "any"})
}
func (locator *Locator) selectOccurrence(occurrence string, nth uint32) *Locator {
	stages := append([]locatorStage(nil), locator.stages...)
	stages[len(stages)-1].occurrence = occurrence
	stages[len(stages)-1].nth = nth
	return &Locator{client: locator.client, stages: stages}
}
func (locator *Locator) Any() *Locator             { return locator.selectOccurrence("any", 0) }
func (locator *Locator) Unique() *Locator          { return locator.selectOccurrence("unique", 0) }
func (locator *Locator) First() *Locator           { return locator.selectOccurrence("first", 0) }
func (locator *Locator) Last() *Locator            { return locator.selectOccurrence("last", 0) }
func (locator *Locator) Nth(index uint32) *Locator { return locator.selectOccurrence("nth", index) }
func (locator *Locator) strictStages() []locatorStage {
	if locator.stages[len(locator.stages)-1].occurrence == "any" {
		return locator.Unique().stages
	}
	return locator.stages
}
func (locator *Locator) Locations() ([]TextMatch, error) {
	matches, err := locator.client.runtime.findLocator(locator.stages)
	return matches, locator.client.guard("locator.locations", err)
}
func (locator *Locator) Location() (TextMatch, error) {
	matches, err := locator.client.runtime.findLocator(locator.strictStages())
	if err != nil {
		return TextMatch{}, locator.client.guard("locator.location", err)
	}
	if len(matches) == 1 {
		return matches[0], nil
	}
	diagnostic := "no match found"
	text, textErr := locator.client.Text(TextOptions{})
	if textErr == nil {
		diagnostic += "\n\nTerminal content:\n" + text
	} else {
		diagnostic += "\n\nTerminal content unavailable: " + textErr.Error()
	}
	return TextMatch{}, locator.client.guard("locator.location", &Error{Kind: AssertionError, Message: diagnostic})
}
func (locator *Locator) Count() (int, error) {
	matches, err := locator.Locations()
	return len(matches), err
}
func (locator *Locator) All() ([]*Locator, error) {
	matches, err := locator.Locations()
	if err != nil {
		return nil, err
	}
	locators := make([]*Locator, len(matches))
	for index := range matches {
		locators[index] = locator
		if locator.stages[len(locator.stages)-1].occurrence == "any" {
			locators[index] = locator.Nth(uint32(index))
		}
	}
	return locators, nil
}
func (locator *Locator) Wait(options LocatorWaitOptions) error {
	if options.State != "" && options.State != Visible && options.State != Hidden {
		return &Error{Kind: UsageError, Message: "locator state must be visible or hidden"}
	}
	return locator.client.wait("locator.wait", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		return locator.client.runtime.waitLocator(locator.stages, options.State == Hidden, timeout)
	})
}
func (locator *Locator) Click(options LocatorClickOptions) error {
	return locator.client.wait("locator.click", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return locator.client.runtime.clickLocator(locator.strictStages(), options)
	})
}
func (locator *Locator) Highlight(options WaitOptions) error {
	return locator.client.wait("locator.highlight", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		return locator.client.runtime.highlightLocator(locator.stages, timeout)
	})
}
func (locator *Locator) Expect(options LocatorExpectOptions) error {
	return locator.client.wait("locator.expect", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return locator.client.runtime.expectLocator(locator.stages, options)
	})
}
