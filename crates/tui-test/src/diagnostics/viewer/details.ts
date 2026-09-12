import { element, get, json, pairs, text, time } from "./dom.js";
import { describeExpectation } from "./expectation.js";
import type { ReportData, Selection } from "./types.js";

export class DetailsPanel {
  constructor(private readonly data: ReportData) {
    const { details, explanation } = data;
    text("operation", explanation.title);
    text("expected", explanation.expected);
    text("actual", explanation.actual);
    text("explanation-note", explanation.note);
    text("failure-timing", `${details.operation.name} / elapsed ${time(details.operation.elapsed_ms)} / timeout ${time(details.operation.timeout_ms)}`);
    text("raw-error", details.summary);
    text("diagnosis", json({ locator: details.locator, comparison: details.comparison, evaluation_transitions: details.evaluation_transitions }));
    for (const hint of details.hints || []) get("hints").append(element("li", `${hint.code}: ${hint.message}`));
  }
  render(selection: Selection) {
    const { operation, frame } = selection;
    const expectation = describeExpectation(operation?.expectation);
    const banner = get("expectation-banner");
    const captured = operation?.expectation && operation.expectation.kind !== "unavailable";
    banner.hidden = !expectation && !selection.isFailure && !operation?.is_assertion;
    banner.dataset.outcome = captured ? (operation.result === "ok" ? "passed" : "failed") : selection.isFailure ? "failed" : "unknown";
    if (expectation) {
      text("summary", `${captured ? (operation?.result === "ok" ? "Passed: " : "Failed: ") : ""}${expectation}`);
      text("summary-observed", selection.isFailure ? this.data.explanation.actual :
        operation?.result === "ok" ? "The selected assertion completed successfully." : "The selected operation did not complete successfully.");
    } else if (selection.isFailure) {
      text("summary", `Expected ${this.data.explanation.expected}`);
      text("summary-observed", this.data.explanation.actual);
    } else {
      text("summary", "Expectation not captured for this operation.");
      text("summary-observed", operation?.safe_summary || "");
    }
    pairs("call-properties", {
      Action: operation?.name || "Frame inspection", Result: operation?.result,
      Expected: expectation || (operation?.is_assertion ? "Not captured in this trace" : "Not an assertion"),
      Summary: operation?.safe_summary, Duration: operation ? time(operation.ended_ms - operation.started_ms) : undefined,
      Started: operation ? time(operation.started_ms) : undefined, Completed: operation ? time(operation.ended_ms) : undefined,
      Screen: frame?.sequence, Cursor: frame && `${frame.cursor.column}, ${frame.cursor.row} / ${frame.cursor.shape} / visible=${frame.cursor.visible}`,
    });
    if (frame) {
      const { cells, grid, svg, ...metadata } = frame;
      text("frame-details", json({ operation: operation || null, frame: metadata }));
    } else {
      text("frame-details", json({ operation, missing_screen_sequence: selection.missingSequence }));
    }
  }
}
