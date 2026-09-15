import { describeExpectation, describeOperation, json, time } from "./format.js";
import { validateReport } from "./schema.js";

/** @import { TraceModel, TraceFrame, TraceAction, TraceAttachment, FrameCell, ViewerState } from "./types.js" */

/** @param {string} source @returns {TraceModel} */
export function parseReport(source) {
  const data = JSON.parse(source);
  validateReport(data);
  const { details, timeline, explanation } = data;
  const { geometry } = timeline;
  const hasFailure = details.outcome !== "passed";
  /** @type {Map<number, number>} */
  const frameIndex = new Map();
  /** @type {TraceFrame[]} */
  const frames = timeline.frames.map((frame, index) => {
    if (frameIndex.has(frame.sequence)) throw new Error(`Duplicate screen sequence: ${frame.sequence}`);
    frameIndex.set(frame.sequence, index);
    if (frame.last_seen_ms < frame.first_seen_ms) throw new Error(`Screen ${frame.sequence}: end precedes start`);
    if (frame.grid.length > frame.size.rows || frame.grid.some((row) =>
      row.length > frame.size.cols || row.some((id) => id >= frame.cells.length))) {
      throw new Error(`Screen ${frame.sequence}: invalid cell grid`);
    }
    const width = frame.size.cols * geometry.cell_width;
    const height = frame.size.rows * geometry.cell_height;
    if (!Number.isFinite(width) || !Number.isFinite(height)) throw new Error(`Screen ${frame.sequence}: invalid dimensions`);
    const { cells, grid, svg, ...evidence } = frame;
    return {
      index, sequence: frame.sequence, started: frame.first_seen_ms, ended: frame.last_seen_ms,
      columns: frame.size.cols, rows: frame.size.rows, width, height,
      viewBox: `${geometry.grid_x} ${geometry.grid_y} ${width} ${height}`,
      cursor: { x: frame.cursor.column, y: frame.cursor.row },
      cursorAppearance: `${frame.cursor.shape}, ${frame.cursor.visible ? "visible" : "hidden"}`,
      text: frame.text, svg, omission: frame.omission, cells, grid, evidence,
      label: `#${frame.sequence} / ${time(frame.first_seen_ms)}`,
      description: `${frame.size.cols} x ${frame.size.rows} / ${time(frame.first_seen_ms)} - ${time(frame.last_seen_ms)} / ${frame.changes.join(", ")}`,
    };
  });
  const actionIds = new Set();
  /** @type {TraceAction[]} */
  const actions = (details.recent_operations ?? []).map((operation) => {
    if (actionIds.has(operation.sequence)) throw new Error(`Duplicate action sequence: ${operation.sequence}`);
    actionIds.add(operation.sequence);
    if (operation.ended_ms < operation.started_ms) throw new Error(`Action ${operation.sequence}: end precedes start`);
    const display = describeOperation(operation);
    const pointer = operation.input?.mouse_position;
    return {
      id: operation.sequence, screen: operation.screen_at_return, frameIndex: frameIndex.get(operation.screen_at_return),
      ...display, expectation: describeExpectation(operation.expectation),
      unavailableExpectation: operation.expectation?.kind === "unavailable",
      code: !!(display.locator || operation.input),
      searchText: `${display.name} ${display.label} ${operation.safe_summary}`.toLowerCase(),
      failed: operation.result !== "ok", assertion: operation.is_assertion ?? false,
      started: operation.started_ms, ended: operation.ended_ms, duration: operation.ended_ms - operation.started_ms,
      result: operation.result, pointer: pointer && { x: pointer.column, y: pointer.row }, evidence: operation,
    };
  });
  const failureAction = hasFailure ? actions.findLast((action) => action.failed && action.screen === timeline.failure_screen_sequence) : undefined;
  const mismatches = (details.locator?.stages ?? [])
    .filter((stage) => stage.stage_index === details.locator?.failure_stage)
    .flatMap((stage) => (stage.mismatches ?? []).map((entry) => ({
      ...entry, stage: stage.stage_index, x: entry.location.column,
      y: entry.location.row - (details.locator?.viewport_origin_y ?? 0),
    })));
  const duration = Math.max(0, ...actions.map((action) => action.ended), ...frames.map((frame) => frame.ended));
  const attachmentNames = new Set();
  /** @type {TraceAttachment[]} */
  const attachments = data.attachments.map((file, id) => {
    if (attachmentNames.has(file.name)) throw new Error(`Duplicate attachment: ${file.name}`);
    attachmentNames.add(file.name);
    const padding = file.data.endsWith("==") ? 2 : file.data.endsWith("=") ? 1 : 0;
    if (file.data.length % 4 !== 0 || /[^A-Za-z0-9+/]/.test(file.data.slice(0, file.data.length - padding))) {
      throw new Error(`Attachment ${file.name}: invalid base64`);
    }
    if (file.data.length / 4 * 3 - padding !== file.bytes) throw new Error(`Attachment ${file.name}: byte count does not match`);
    const mime = file.name.endsWith(".svg") ? "image/svg+xml" : file.name.endsWith(".json") ? "application/json" : "text/plain";
    const limit = 512 * 1024;
    const truncated = file.bytes > limit;
    const decoder = new TextDecoder();
    /** @type {BlobPart[]} */
    const parts = [];
    let preview = "";
    let offset = 0;
    for (let start = 0; start < file.data.length; start += 64 * 1024) {
      const decoded = atob(file.data.slice(start, start + 64 * 1024));
      const bytes = new Uint8Array(decoded.length);
      for (let index = 0; index < decoded.length; index++) bytes[index] = decoded.charCodeAt(index);
      parts.push(bytes);
      if (mime !== "image/svg+xml" && offset < limit) {
        preview += decoder.decode(bytes.subarray(0, limit - offset), { stream: true });
      }
      offset += bytes.length;
    }
    if (!truncated && mime !== "image/svg+xml") preview += decoder.decode();
    return { id, name: file.name, byteLength: file.bytes, blob: new Blob(parts, { type: mime }), mime, truncated, preview };
  });
  const runtime = details.runtime;
  const history = details.terminal?.screen_history;
  return {
    title: `tui-test: ${runtime?.session_name || details.operation.name}`,
    hasFailure, duration, frames, actions, geometry, mismatches,
    failureFrameIndex: frameIndex.get(timeline.failure_screen_sequence),
    failureActionId: failureAction?.id, failureScreen: timeline.failure_screen_sequence,
    summary: { Session: runtime?.session_name || "Not captured", Emulator: runtime?.backend || "Not captured", Duration: time(duration) },
    metadata: {
      properties: {
        Name: runtime?.session_name, Emulator: runtime?.backend, Shell: runtime?.shell || "Direct program",
        "Terminal size": details.terminal && `${details.terminal.size.cols} x ${details.terminal.size.rows}`,
        Title: details.terminal?.title, "Process ID": details.process?.pid, "Process state": details.process?.state,
        Platform: runtime && `${runtime.target_os} / ${runtime.target_arch}`, "tui-test version": runtime?.tui_test_version,
        "Assertion timeout": time(details.operation.timeout_ms), "Assertion elapsed": time(details.operation.elapsed_ms),
      },
      timeouts: runtime?.timeouts ? Object.fromEntries(Object.entries(runtime.timeouts).map(([name, ms]) => [name, time(ms)])) : { Defaults: "Not captured" },
      retention: `${frames.length} retained frames; ${history?.dropped_screen_count ?? 0} sampled screens evicted; ` +
        `${history?.dropped_checkpoint_count ?? 0} checkpoints evicted or omitted. ` +
        "Frames are bounded observations, not every PTY write. Missing frames are never substituted. " +
        "Playback steps retained frames every 400 ms. Passing and failing assertion operands are retained up to 8 KiB per operation.",
      evidence: json({ ...runtime, process: details.process, context: details.context, recording: details.recording,
        truncated: details.truncated, signature: details.signature, files: data.files, errors: data.errors }),
    },
    result: {
      ...explanation, locator: failureAction?.locator, error: details.summary,
      note: hasFailure ? explanation.note : "Select an action to inspect its inputs and return screen.",
      timing: hasFailure ? `${details.operation.name} / elapsed ${time(details.operation.elapsed_ms)} / timeout ${time(details.operation.timeout_ms)}`
        : `${actions.length} retained actions / elapsed ${time(details.operation.elapsed_ms)}`,
      hints: (details.hints ?? []).map((hint) => `${hint.code}: ${hint.message}`),
      evidence: json({ locator: details.locator, comparison: details.comparison, evaluation_transitions: details.evaluation_transitions }),
    },
    attachments,
    unavailableFiles: data.files.filter((file) => file.status !== "written").map((file) => `${file.path}: ${file.status} (${file.reason || "not available"})`),
    recordingNote: attachments.some((file) => file.name === "session.cast")
      ? "The complete included session.cast is embedded and downloadable for continuous replay in an asciicast player."
      : `Recording not included: ${details.recording?.reason || "inclusion is opt-in"}. Retained frame inspection does not require a recording.`,
  };
}

/** @param {TraceFrame | undefined} frame @param {number} x @param {number} y */
export function cellAt(frame, x, y) {
  if (!frame || !Number.isInteger(x) || !Number.isInteger(y) || x < 0 || y < 0 || x >= frame.columns || y >= frame.rows) return undefined;
  const id = frame.grid[y]?.[x];
  return id === undefined ? undefined : frame.cells[id];
}

/** @param {TraceModel} model @param {ViewerState} state */
export function selection(model, state) {
  const frame = state.frameIndex === undefined ? undefined : model.frames[state.frameIndex];
  const action = model.actions.find((action) => action.id === state.actionId);
  const sharesFailure = model.hasFailure && !!frame && frame.index === model.failureFrameIndex;
  const isFailure = sharesFailure && (!action || action.id === model.failureActionId);
  return { frame, action, sharesFailure, isFailure, mismatches: isFailure ? model.mismatches : [] };
}

/** @param {TraceModel} model @param {ViewerState} state */
export function visibleActions(model, state) {
  const filter = state.filter.toLowerCase();
  return model.actions.filter((action) => (!state.assertionsOnly || action.assertion || action.failed) && action.searchText.includes(filter));
}

/** @param {TraceModel} model @param {ViewerState} state */
export function inspectCell(model, state) {
  const { frame, mismatches } = selection(model, state);
  const cell = state.cell && cellAt(frame, state.cell.x, state.cell.y);
  if (!cell || !frame || !state.cell) return undefined;
  const { x, y } = state.cell;
  const previous = cellAt(model.frames[frame.index - 1], x, y);
  const leadColumn = cell.width === 0 && x > 0 && cellAt(frame, x - 1, y)?.width === 2 ? x - 1 : undefined;
  return { frame, cell, x, y, previous, leadColumn, mismatches: mismatches.filter((entry) => entry.x === x && entry.y === y) };
}
