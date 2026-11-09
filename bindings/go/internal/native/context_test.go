package native

import (
	"encoding/json"
	"testing"
)

func TestContextOverridesRemainPerCall(t *testing.T) {
	const defaults = `{"name":"test","diagnostic_context":{"suite":"default"},"screen_history_limit":5}`
	diagnostics := map[string]string{"phase": "spawn"}
	options := &ExecutionContext{
		DiagnosticContext:  &diagnostics,
		OperationName:      pointerTo("custom"),
		ScreenHistoryLimit: pointerTo(uint16(0)),
	}
	encoded, err := mergeContext(defaults, options)
	if err != nil {
		t.Fatal(err)
	}
	var actual struct {
		Name               string
		DiagnosticContext  map[string]string `json:"diagnostic_context"`
		ScreenHistoryLimit uint16            `json:"screen_history_limit"`
		OperationName      string            `json:"operation_name"`
	}
	if err := json.Unmarshal([]byte(encoded), &actual); err != nil {
		t.Fatal(err)
	}
	if actual.Name != "test" || actual.DiagnosticContext["phase"] != "spawn" || actual.ScreenHistoryLimit != 0 || actual.OperationName != "custom" {
		t.Fatalf("%+v", actual)
	}
	unchanged, err := mergeContext(defaults, nil)
	if err != nil || unchanged != defaults || len(diagnostics) != 1 || diagnostics["phase"] != "spawn" {
		t.Fatal("mutated context defaults")
	}
	empty := map[string]string{}
	encoded, err = mergeContext(defaults, &ExecutionContext{DiagnosticContext: &empty})
	if err != nil {
		t.Fatal(err)
	}
	var values map[string]json.RawMessage
	if err := json.Unmarshal([]byte(encoded), &values); err != nil {
		t.Fatal(err)
	}
	if string(values["diagnostic_context"]) != "{}" {
		t.Fatalf("explicit empty map: %s", encoded)
	}
}
