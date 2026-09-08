package tuitest

import "time"

type locatorNode struct {
	kind       string
	text       *string
	regex      *bool
	whitespace *Whitespace
	style      *TextStyle
	link       *string
	full       *bool
	direction  *Direction
	within     *uint32
	left       *uint32
	right      *uint32
	input      *uint32
	has        *uint32
	hasNot     *uint32
	occurrence any
}

type locatorQuery struct {
	nodes []locatorNode
	root  uint32
}

type LocatorFilterOptions struct {
	Has    *Locator
	HasNot *Locator
}

type LinkSelectorOptions struct {
	Full      bool
	Direction Direction
}

// Locator is an immutable query, resolved against the current grid on each operation.
// Invalid selector options are reported when an operation resolves the locator.
type Locator struct {
	client *Client
	query  locatorQuery
}

func (client *Client) GetByText(text string, options TextSelectorOptions) *Locator {
	return newTextLocator(client, nil, text, options)
}

func (client *Client) GetByStyle(style TextStyle, options StyleSelectorOptions) *Locator {
	return newStyleLocator(client, nil, style, options)
}

func (client *Client) GetByLink(uri string, options LinkSelectorOptions) *Locator {
	return newLinkLocator(client, nil, uri, options)
}

func cloneLocatorQuery(source locatorQuery) locatorQuery {
	return locatorQuery{nodes: append([]locatorNode(nil), source.nodes...), root: source.root}
}

func appendLocatorNode(parent *locatorQuery, node locatorNode) locatorQuery {
	query := locatorQuery{}
	if parent != nil {
		query = cloneLocatorQuery(*parent)
		node.within = Ptr(query.root)
	}
	query.root = uint32(len(query.nodes))
	query.nodes = append(query.nodes, node)
	return query
}

func newTextLocator(client *Client, parent *locatorQuery, text string, options TextSelectorOptions) *Locator {
	whitespace := options.Whitespace
	if whitespace == "" {
		whitespace = Exact
	}
	direction := options.Direction
	if direction == "" {
		direction = Within
	}
	node := locatorNode{kind: "text", text: Ptr(text), regex: Ptr(options.Regex), whitespace: Ptr(whitespace), full: Ptr(options.Full), direction: Ptr(direction), occurrence: "any"}
	return &Locator{client: client, query: appendLocatorNode(parent, node)}
}

func newStyleLocator(client *Client, parent *locatorQuery, style TextStyle, options StyleSelectorOptions) *Locator {
	style = cloneTextStyle(style)
	direction := options.Direction
	if direction == "" {
		direction = Within
	}
	node := locatorNode{kind: "style", style: &style, full: Ptr(options.Full), direction: Ptr(direction), occurrence: "any"}
	return &Locator{client: client, query: appendLocatorNode(parent, node)}
}

func newLinkLocator(client *Client, parent *locatorQuery, uri string, options LinkSelectorOptions) *Locator {
	direction := options.Direction
	if direction == "" {
		direction = Within
	}
	node := locatorNode{kind: "link", link: Ptr(uri), full: Ptr(options.Full), direction: Ptr(direction), occurrence: "any"}
	return &Locator{client: client, query: appendLocatorNode(parent, node)}
}

func cloneTextStyle(style TextStyle) TextStyle {
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
	return style
}

func (locator *Locator) GetByText(text string, options TextSelectorOptions) *Locator {
	return newTextLocator(locator.client, &locator.query, text, options)
}

func (locator *Locator) GetByStyle(style TextStyle, options StyleSelectorOptions) *Locator {
	return newStyleLocator(locator.client, &locator.query, style, options)
}

func (locator *Locator) GetByLink(uri string, options LinkSelectorOptions) *Locator {
	return newLinkLocator(locator.client, &locator.query, uri, options)
}

func offsetReference(reference *uint32, offset uint32) *uint32 {
	if reference == nil {
		return nil
	}
	return Ptr(*reference + offset)
}

func appendLocatorOperand(query *locatorQuery, operand locatorQuery) uint32 {
	offset := uint32(len(query.nodes))
	for _, source := range operand.nodes {
		source.within = offsetReference(source.within, offset)
		source.left = offsetReference(source.left, offset)
		source.right = offsetReference(source.right, offset)
		source.input = offsetReference(source.input, offset)
		source.has = offsetReference(source.has, offset)
		source.hasNot = offsetReference(source.hasNot, offset)
		query.nodes = append(query.nodes, source)
	}
	return operand.root + offset
}

func (locator *Locator) operand(other *Locator) (locatorQuery, error) {
	if other == nil || other.client != locator.client {
		return locatorQuery{}, &Error{Kind: UsageError, Message: "locator operands must belong to the same terminal owner"}
	}
	return other.query, nil
}

func (locator *Locator) combine(kind string, other *Locator) (*Locator, error) {
	operand, err := locator.operand(other)
	if err != nil {
		return nil, err
	}
	query := cloneLocatorQuery(locator.query)
	left := query.root
	right := appendLocatorOperand(&query, operand)
	query.root = uint32(len(query.nodes))
	query.nodes = append(query.nodes, locatorNode{kind: kind, left: Ptr(left), right: Ptr(right), occurrence: "any"})
	return &Locator{client: locator.client, query: query}, nil
}

func (locator *Locator) And(other *Locator) (*Locator, error) { return locator.combine("and", other) }
func (locator *Locator) Or(other *Locator) (*Locator, error)  { return locator.combine("or", other) }

func (locator *Locator) Filter(options LocatorFilterOptions) (*Locator, error) {
	if options.Has == nil && options.HasNot == nil {
		return nil, &Error{Kind: UsageError, Message: "filter requires has or hasNot"}
	}
	query := cloneLocatorQuery(locator.query)
	input := query.root
	node := locatorNode{kind: "filter", input: Ptr(input), occurrence: "any"}
	if options.Has != nil {
		operand, err := locator.operand(options.Has)
		if err != nil {
			return nil, err
		}
		node.has = Ptr(appendLocatorOperand(&query, operand))
	}
	if options.HasNot != nil {
		operand, err := locator.operand(options.HasNot)
		if err != nil {
			return nil, err
		}
		node.hasNot = Ptr(appendLocatorOperand(&query, operand))
	}
	query.root = uint32(len(query.nodes))
	query.nodes = append(query.nodes, node)
	return &Locator{client: locator.client, query: query}, nil
}

func (locator *Locator) selectOccurrence(occurrence any) *Locator {
	query := cloneLocatorQuery(locator.query)
	query.nodes[query.root].occurrence = occurrence
	return &Locator{client: locator.client, query: query}
}

func (locator *Locator) Any() *Locator    { return locator.selectOccurrence("any") }
func (locator *Locator) Unique() *Locator { return locator.selectOccurrence("unique") }
func (locator *Locator) First() *Locator  { return locator.selectOccurrence("first") }
func (locator *Locator) Last() *Locator   { return locator.selectOccurrence("last") }
func (locator *Locator) Nth(index uint32) *Locator {
	return locator.selectOccurrence(map[string]uint32{"nth": index})
}

func (locator *Locator) Locations() ([]TextMatch, error) {
	matches, err := locator.client.runtime.findLocator(locator.query, false)
	return matches, locator.client.guard("locator.locations", err)
}

func (locator *Locator) Location() (TextMatch, error) {
	matches, err := locator.client.runtime.findLocator(locator.query, true)
	if err != nil {
		return TextMatch{}, locator.client.guard("locator.location", err)
	}
	if len(matches) != 1 {
		return TextMatch{}, &Error{Kind: InternalError, Message: "locator.location: native returned an invalid match count"}
	}
	return matches[0], nil
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
		if locator.query.nodes[locator.query.root].occurrence == "any" {
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
		return locator.client.runtime.waitLocator(locator.query, options.State == Hidden, timeout)
	})
}

func (locator *Locator) Click(options LocatorClickOptions) error {
	return locator.client.wait("locator.click", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return locator.client.runtime.clickLocator(locator.query, options)
	})
}

func (locator *Locator) Highlight(options WaitOptions) error {
	return locator.client.wait("locator.highlight", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		return locator.client.runtime.highlightLocator(locator.query, timeout)
	})
}

func (locator *Locator) Expect(options LocatorExpectOptions) error {
	return locator.client.wait("locator.expect", options.Timeout, locator.client.options.Timeouts.Text, func(timeout *time.Duration) error {
		options.Timeout = timeout
		return locator.client.runtime.expectLocator(locator.query, options)
	})
}
