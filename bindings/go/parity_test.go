package tuitest

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestParityLocatorTransport(t *testing.T) {
	client := &Client{}
	scope := &TextScope{After: &TextAnchor{Text: "Start", Regex: true, Nth: Ptr(uint32(1))}, Before: &TextAnchor{Text: "End"}}
	style := TextStyle{Bold: Ptr(true)}
	text := client.GetByText("Save", TextSelectorOptions{Scope: scope}).WithStyle(style)
	scope.After.Text = "changed"
	*style.Bold = false
	composite, err := text.Or(client.GetByLink("test:save", LinkSelectorOptions{}))
	if err != nil {
		t.Fatal(err)
	}
	relative, err := composite.RelativeTo(client.GetByText("Header", TextSelectorOptions{}), After)
	if err != nil {
		t.Fatal(err)
	}
	relative = relative.WithStyle(TextStyle{Italic: Ptr(false)})
	value := engineLocatorQuery(relative.query)
	node := value.Nodes[value.Root]
	if node.Kind != "or" || *node.Within != 0 || *node.MatchStyle.Italic {
		t.Fatalf("%+v", node)
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range []string{`"text":"Start"`, `"occurrence":{"nth":1}`, `"occurrence":"unique"`, `"match_style":{"bold":true}`} {
		if !strings.Contains(string(encoded), field) {
			t.Fatalf("missing %s in %s", field, encoded)
		}
	}
	if composite.query.nodes[composite.query.root].within != nil {
		t.Fatal("mutated composite")
	}
	if _, err := composite.RelativeTo((&Client{}).GetByText("Header", TextSelectorOptions{}), Within); err == nil {
		t.Fatal("accepted another owner")
	}
}

func TestParitySpawnContextConversion(t *testing.T) {
	context := &ExecutionContext{
		OperationName: Ptr("custom"), DiagnosticContext: map[string]string{"case": "save"},
		Trace: &TraceOptions{}, ScreenHistoryLimit: Ptr(uint16(5)),
	}
	options := SpawnOptions{Context: context, Recording: &AutomaticRecording{Directory: "captures"}}
	value := engineSpawnOptions(options)
	if value.Recording.Directory != "captures" || *value.Context.TraceMode != "off" || *value.Context.OperationName != "custom" {
		t.Fatalf("%+v", value)
	}
	if context.Trace.Mode != "" {
		t.Fatal("mutated caller context")
	}
}

func TestParityRenderingDefaultsCopyNestedValues(t *testing.T) {
	style := &RenderStyle{
		Font:           FontFamilies{Files: []string{"original.ttf"}, BoldItalic: Ptr("monospace")},
		CanvasPadding:  Padding{Uniform: Ptr(uint32(0))},
		ContentPadding: Padding{Left: Ptr(uint32(0))},
		Window:         WindowStyle{TitleBar: Ptr(false)},
		Border:         BorderStyle{Radius: Ptr(0.0)},
		Shadow:         ShadowStyle{Enabled: Ptr(false)},
		Cursor:         RecordingCursorStyle{Blink: Ptr(false), BlinkIntervalMS: Ptr(uint64(250))},
	}
	client, err := New("rendering-defaults", ClientOptions{Style: style})
	if err != nil {
		t.Fatal(err)
	}
	style.Font.Files[0] = "changed.ttf"
	*style.Font.BoldItalic = "serif"
	*style.CanvasPadding.Uniform = 10
	*style.ContentPadding.Left = 10
	*style.Window.TitleBar = true
	*style.Border.Radius = 8
	*style.Shadow.Enabled = true
	*style.Cursor.Blink = true
	*style.Cursor.BlinkIntervalMS = 500
	options, err := client.spawnOptions(SpawnOptions{})
	if err != nil {
		t.Fatal(err)
	}
	actual := options.Style
	if actual.Font.Files[0] != "original.ttf" || *actual.Font.BoldItalic != "monospace" ||
		*actual.CanvasPadding.Uniform != 0 || *actual.ContentPadding.Left != 0 ||
		*actual.Window.TitleBar || *actual.Border.Radius != 0 || *actual.Shadow.Enabled ||
		*actual.Cursor.Blink || *actual.Cursor.BlinkIntervalMS != 250 {
		t.Fatal("caller mutation changed nested rendering defaults")
	}
	if actual.FontSize != nil || actual.Window.Background != nil || actual.ContentPadding.Top != nil {
		t.Fatal("omitted rendering defaults became explicit")
	}
}
