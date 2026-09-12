import { element, get, json, pairs, text, time } from "./dom.js";
import type { TraceModel } from "./model.js";

export function renderMetadata(model: TraceModel) {
  const { details, files, errors } = model.data;
  const runtime = details.runtime;
  document.title = `tui-test: ${runtime?.session_name || details.operation.name}`;
  for (const [label, value] of [
    ["Session", runtime?.session_name || "Not captured"],
    ["Emulator", runtime?.backend || "Not captured"],
    ["Duration", time(model.duration)],
  ]) {
    const item = element("span", "", "metadata-item");
    item.append(element("span", `${label}: `, "metadata-label"), element("span", value, "metadata-value"));
    item.title = `${label}: ${value}`;
    get("session-summary").append(item, document.createTextNode(" "));
  }
  pairs("session-properties", {
    Name: runtime?.session_name, Emulator: runtime?.backend, Shell: runtime?.shell || "Direct program",
    "Terminal size": details.terminal ? `${details.terminal.size.cols} x ${details.terminal.size.rows}` : undefined,
    Title: details.terminal?.title, "Process ID": details.process?.pid, "Process state": details.process?.state,
    Platform: runtime && `${runtime.target_os} / ${runtime.target_arch}`,
    "tui-test version": runtime?.tui_test_version, "Assertion timeout": time(details.operation.timeout_ms),
    "Assertion elapsed": time(details.operation.elapsed_ms),
  });
  pairs("timeout-properties", runtime?.timeouts
    ? Object.fromEntries(Object.entries(runtime.timeouts).map(([name, ms]) => [name, time(ms)]))
    : { Defaults: "Not captured" });
  const history = details.terminal?.screen_history;
  text("retention", `${model.frames.length} retained frames; ${history?.dropped_screen_count || 0} sampled screens evicted; ` +
    `${history?.dropped_checkpoint_count || 0} checkpoints evicted or omitted. ` +
    "Frames are bounded observations, not every PTY write. Missing frames are never substituted. " +
    "Playback steps retained frames every 400 ms. Passing and failing assertion operands are retained up to 8 KiB per operation.");
  text("runtime", json({ ...runtime, process: details.process, context: details.context,
    recording: details.recording, truncated: details.truncated, signature: details.signature, files, errors }));
}
