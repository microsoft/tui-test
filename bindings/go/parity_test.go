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
