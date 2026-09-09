(() => {
  "use strict";
  const $ = (id) => document.getElementById(id);
  const text = (id, value) => { $(id).textContent = value; };
  const json = (value) => JSON.stringify(value, null, 2);
  const time = (ms) => ms === undefined ? "Not captured" : `${ms} ms`;
  const element = (tag, value = "", className = "") => {
    const node = document.createElement(tag);
    node.textContent = value;
    node.className = className;
    return node;
  };
  const showError = (message) => {
    text("report-error", `Report viewer error: ${message}. Captured evidence remains in the embedded report-data block.`);
    $("report-error").hidden = false;
  };
  window.addEventListener("error", (event) => showError(event.message));
  window.addEventListener("unhandledrejection", (event) => showError(String(event.reason)));

  const { details, timeline, files, errors, explanation, attachments } = JSON.parse($("report-data").textContent);
  const frames = timeline.frames;
  const operations = details.recent_operations || [];
  const frameIndex = new Map(frames.map((frame, i) => [frame.sequence, i]));
  const failureIndex = frameIndex.get(timeline.failure_screen_sequence);
  const failureOperation = operations.findLast((op) => op.result !== "ok" &&
    op.screen_at_return === timeline.failure_screen_sequence) || null;
  const geometry = timeline.geometry;
  const images = new Map();
  const downloads = new Map();
  let index = -1;
  let selectedOperation = null;
  let selectedCell = null;
  let timer = null;
  const isFailureView = () => index === failureIndex && (!selectedOperation || selectedOperation === failureOperation);
  const visibleOperations = () => operations.filter((op) =>
    (!$("assertions-only").checked || op.is_assertion || op.result !== "ok") &&
    `${op.name} ${op.safe_summary}`.toLowerCase().includes($("action-filter").value.toLowerCase()));
  const failureMismatches = (details.locator?.stages || []).flatMap((stage) =>
    (stage.mismatches || []).map((mismatch) => ({
      ...mismatch, stage: stage.stage_index, x: mismatch.location.column,
      y: mismatch.location.row - details.locator.viewport_origin_y,
    })));
  const mismatches = () => isFailureView() ? failureMismatches : [];

  function setTab(id) {
    const selected = $(id);
    for (const button of document.querySelectorAll(`[data-group="${selected.dataset.group}"]`)) {
      const active = button === selected;
      button.setAttribute("aria-selected", String(active));
      button.tabIndex = active ? 0 : -1;
      $(button.getAttribute("aria-controls")).hidden = !active;
    }
  }
  for (const button of document.querySelectorAll("[role=tab]")) {
    button.addEventListener("click", () => setTab(button.id));
    button.addEventListener("keydown", (event) => {
      const tabs = Array.from(document.querySelectorAll(`[data-group="${button.dataset.group}"]`));
      const offset = { ArrowLeft: -1, ArrowRight: 1, Home: -tabs.indexOf(button), End: tabs.length - 1 - tabs.indexOf(button) }[event.key];
      if (offset === undefined) return;
      event.preventDefault();
      const next = tabs[(tabs.indexOf(button) + offset + tabs.length) % tabs.length];
      setTab(next.id);
      next.focus();
    });
  }

  function pairs(id, values, colors = {}) {
    $(id).replaceChildren();
    for (const [key, value] of Object.entries(values)) {
      const content = element("dd", value == null ? "Not captured" : String(value));
      if (/^#[0-9a-f]{6}$/i.test(colors[key] || "")) {
        const swatch = element("span", "", "swatch");
        swatch.style.backgroundColor = colors[key];
        content.prepend(swatch);
      }
      $(id).append(element("dt", key), content);
    }
  }

  const runtime = details.runtime || {};
  const duration = Math.max(0, ...operations.map((op) => op.ended_ms), ...frames.map((frame) => frame.last_seen_ms));
  document.title = `tui-test: ${runtime.session_name || details.operation.name}`;
  for (const [label, value] of [
    ["Session", runtime.session_name || "Not captured"], ["Emulator", runtime.backend || "Not captured"],
    ["Duration", time(duration)], ["Assertion timeout", time(details.operation.timeout_ms)],
  ]) {
    const item = element("span");
    item.append(element("b", `${label}:`), document.createTextNode(value));
    item.title = `${label}: ${value}`;
    $("session-summary").append(item);
  }
  pairs("session-properties", {
    Name: runtime.session_name, Emulator: runtime.backend, Shell: runtime.shell || "Direct program",
    "Terminal size": details.terminal ? `${details.terminal.size.cols} x ${details.terminal.size.rows}` : undefined,
    Title: details.terminal?.title, "Process ID": details.process?.pid, "Process state": details.process?.state,
    Platform: runtime.target_os && `${runtime.target_os} / ${runtime.target_arch}`,
    "tui-test version": runtime.tui_test_version, "Assertion timeout": time(details.operation.timeout_ms),
    "Assertion elapsed": time(details.operation.elapsed_ms),
  });
  pairs("timeout-properties", runtime.timeouts
    ? Object.fromEntries(Object.entries(runtime.timeouts).map(([name, ms]) => [name, time(ms)]))
    : { Defaults: "Not captured" });
  const history = details.terminal?.screen_history;
  text("retention", `${frames.length} retained frames; ${history?.dropped_screen_count || 0} sampled screens evicted; ` +
    `${history?.dropped_checkpoint_count || 0} checkpoints evicted or omitted. ` +
    "Frames are bounded observations, not every PTY write. Missing frames are never substituted. " +
    "Playback steps retained frames every 400 ms; passing checkpoints capture operation completion and failures use the pinned evaluation.");
  text("runtime", json({ ...runtime, process: details.process, context: details.context,
    recording: details.recording, truncated: details.truncated, signature: details.signature, files, errors }));
  text("operation", explanation.title);
  text("summary", `Expected ${explanation.expected}`);
  text("summary-observed", explanation.actual);
  text("expected", explanation.expected);
  text("actual", explanation.actual);
  text("explanation-note", explanation.note);
  text("failure-timing", `${details.operation.name} / elapsed ${time(details.operation.elapsed_ms)} / timeout ${time(details.operation.timeout_ms)}`);
  text("raw-error", details.summary);
  text("diagnosis", json({ locator: details.locator, comparison: details.comparison, evaluation_transitions: details.evaluation_transitions }));
  for (const hint of details.hints || []) $("hints").append(element("li", `${hint.code}: ${hint.message}`));

  function image(frame) {
    if (!frame.svg) return {};
    if (!images.has(frame.sequence)) {
      // Crop only the presentation; the downloadable SVG keeps the original screenshot.
      const document = new DOMParser().parseFromString(frame.svg, "image/svg+xml");
      if (document.querySelector("parsererror")) {
        const result = { error: "Captured SVG could not be rendered. Terminal text and cell metadata are still available." };
        images.set(frame.sequence, result);
        return result;
      }
      const root = document.documentElement;
      root.setAttribute("viewBox", `${geometry.grid_x} ${geometry.grid_y} ${frame.size.cols * geometry.cell_width} ${frame.size.rows * geometry.cell_height}`);
      root.setAttribute("width", frame.size.cols * geometry.cell_width);
      root.setAttribute("height", frame.size.rows * geometry.cell_height);
      images.set(frame.sequence, { url: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(root))}` });
    }
    return images.get(frame.sequence);
  }

  for (let tick = 0; tick <= 5; tick++) $("time-scale").append(element("span", time(Math.round(duration * tick / 5))));
  for (const op of operations) {
    const bar = element("button", "", `action-bar${op.result !== "ok" ? " failed" : ""}`);
    bar.style.left = `${Math.min(99.5, op.started_ms / Math.max(1, duration) * 100)}%`;
    bar.style.width = `${Math.max(.4, (op.ended_ms - op.started_ms) / Math.max(1, duration) * 100)}%`;
    bar.dataset.operation = op.sequence;
    bar.title = `${op.name}: ${time(op.ended_ms - op.started_ms)} (${op.result})`;
    bar.setAttribute("aria-label", bar.title);
    bar.addEventListener("click", () => goOperation(op));
    $("action-timeline").append(bar);
  }
  frames.forEach((frame, i) => {
    const button = element("button", "", "thumbnail");
    button.dataset.frame = i;
    button.setAttribute("aria-label", `Screen ${frame.sequence} at ${time(frame.first_seen_ms)}`);
    const screenshot = image(frame);
    if (screenshot.url) {
      const thumbnail = element("img");
      thumbnail.src = screenshot.url;
      thumbnail.alt = "";
      button.append(thumbnail);
    }
    button.append(element("span", `#${frame.sequence} / ${time(frame.first_seen_ms)}`));
    button.addEventListener("click", () => { stop(); showFrame(i); });
    $("filmstrip").append(button);
  });

  function renderActions() {
    $("points").replaceChildren();
    const visible = visibleOperations();
    for (const op of visible) {
      const button = element("button", "", `point${op.result !== "ok" ? " failed" : ""}`);
      button.dataset.operation = op.sequence;
      button.setAttribute("aria-current", String(op === selectedOperation));
      button.title = `${op.safe_summary} / screen ${op.screen_at_return}`;
      const label = element("span");
      label.append(element("span", `${op.sequence}. ${op.name}`, "name"),
        element("span", op.safe_summary, "description"));
      button.append(element("span", "", "status"), label, element("span",
        `${op.result === "ok" ? "PASS" : "FAIL"}\n${time(op.ended_ms - op.started_ms)}` +
        (frameIndex.has(op.screen_at_return) ? "" : "\nnot retained"), "result"));
      button.addEventListener("click", () => goOperation(op));
      $("points").append(button);
    }
    if (!visible.length) $("points").append(element("p", "No matching actions.", "muted"));
    updateActionSelection();
  }
  function updateActionSelection() {
    const visible = visibleOperations();
    const position = visible.indexOf(selectedOperation);
    $("previous-point").disabled = !visible.length || position === 0;
    $("next-point").disabled = !visible.length || position === visible.length - 1;
    for (const bar of $("action-timeline").children) bar.setAttribute("aria-current", String(Number(bar.dataset.operation) === selectedOperation?.sequence));
    for (const button of $("points").querySelectorAll("[data-operation]")) {
      button.setAttribute("aria-current", String(Number(button.dataset.operation) === selectedOperation?.sequence));
    }
  }

  function stop() {
    clearInterval(timer);
    timer = null;
    text("play", "Play to next point");
  }
  function goOperation(op) {
    stop();
    showFrame(frameIndex.get(op.screen_at_return), op, op.screen_at_return);
  }
  function zoom() {
    const frame = frames[index];
    if (!frame) return;
    $("viewport").classList.toggle("fit", $("zoom").value === "fit");
    $("terminal").style.width = `${frame.size.cols * geometry.cell_width * ($("zoom").value === "fit" ? 1 : Number($("zoom").value))}px`;
  }

  function showFrame(next, op = null, missingSequence = null) {
    index = next ?? -1;
    selectedOperation = op;
    selectedCell = null;
    const frame = frames[index];
    const screenshot = frame ? image(frame) : {};
    $("terminal").hidden = true;
    $("screen-text").hidden = true;
    $("frame-notice").hidden = !frame?.omission && !screenshot.error;
    $("failure-banner").hidden = !isFailureView();
    updateActionSelection();
    text("frame-heading", op?.name || "Terminal snapshot");
    text("frame-meta", frame ? `Screen ${frame.sequence}${index === failureIndex ? (isFailureView() ? " / failure" : " / also used by failure") : ""} / ` +
      `${frame.size.cols} x ${frame.size.rows} / ${time(frame.first_seen_ms)} - ${time(frame.last_seen_ms)} / ${frame.changes.join(", ")}` :
      `Screen ${missingSequence ?? "unknown"} was not retained. No substitute frame is shown.`);
    text("frame-count", frame ? `${index + 1} / ${frames.length}` : "Unavailable");
    $("previous-frame").disabled = index <= 0;
    $("next-frame").disabled = !frames.length || index >= frames.length - 1;
    $("play").disabled = index < 0 || index >= frames.length - 1;
    $("inspect").disabled = !frame?.grid.length;
    for (const thumbnail of $("filmstrip").children) thumbnail.setAttribute("aria-current", String(Number(thumbnail.dataset.frame) === index));
    pairs("call-properties", {
      Action: op?.name || "Frame inspection", Result: op?.result,
      Summary: op?.safe_summary, Duration: op ? time(op.ended_ms - op.started_ms) : undefined,
      Started: op ? time(op.started_ms) : undefined, Completed: op ? time(op.ended_ms) : undefined,
      Screen: frame?.sequence, Cursor: frame && `${frame.cursor.column}, ${frame.cursor.row} / ${frame.cursor.shape} / visible=${frame.cursor.visible}`,
    });
    if (frame) {
      $("frame-slider").value = index;
      $("column").max = frame.size.cols - 1;
      $("row").max = frame.size.rows - 1;
      const { cells, grid, svg, ...metadata } = frame;
      text("frame-details", json({ operation: op, frame: metadata }));
      text("frame-notice", frame.omission || screenshot.error || "");
      if (screenshot.url) {
        $("overlay").setAttribute("viewBox", `${geometry.grid_x} ${geometry.grid_y} ${frame.size.cols * geometry.cell_width} ${frame.size.rows * geometry.cell_height}`);
        $("screen-image").src = screenshot.url;
        $("terminal").hidden = false;
        zoom();
      } else {
        text("screen-text", frame.text);
        $("screen-text").hidden = false;
      }
      const mismatch = mismatches().find((entry) => cellAt(frame, entry.x, entry.y));
      inspect(mismatch?.x ?? Math.min(frame.cursor.column, frame.size.cols - 1),
        mismatch?.y ?? Math.min(frame.cursor.row, frame.size.rows - 1));
    } else {
      text("frame-details", json({ operation: op, missing_screen_sequence: missingSequence }));
      text("screen-text", "This checkpoint's screen was evicted or could not be captured. Select another action or jump to failure.");
      $("screen-text").hidden = false;
      inspect(-1, -1);
    }
  }

  function cellAt(frame, x, y) {
    const id = frame?.grid[y]?.[x];
    return id === undefined ? undefined : frame.cells[id];
  }
  function drawOverlay() {
    $("overlay").replaceChildren();
    if (!frames[index]?.svg || images.get(frames[index].sequence)?.error) return;
    const rect = (x, y, className) => {
      const node = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      for (const [name, value] of Object.entries({
        x: geometry.grid_x + x * geometry.cell_width, y: geometry.grid_y + y * geometry.cell_height,
        width: geometry.cell_width, height: geometry.cell_height, class: className,
      })) node.setAttribute(name, value);
      $("overlay").append(node);
    };
    const locations = new Set(mismatches().filter((entry) => cellAt(frames[index], entry.x, entry.y))
      .map((entry) => `${entry.x},${entry.y}`));
    for (const location of locations) rect(...location.split(",").map(Number), "mismatch-cell");
    if (selectedCell) rect(selectedCell.x, selectedCell.y, "selected-cell");
  }
  function inspect(x, y, activate = false) {
    const frame = frames[index];
    const cell = Number.isInteger(x) && Number.isInteger(y) ? cellAt(frame, x, y) : undefined;
    selectedCell = cell ? { x, y } : null;
    $("cell-properties").replaceChildren();
    $("cell-mismatches").replaceChildren();
    $("cell-mismatches").hidden = true;
    text("cell-json", "");
    drawOverlay();
    if (activate) setTab("cell-tab");
    if (!cell) { text("cell-heading", "Cell metadata is unavailable at these coordinates."); return; }
    $("column").value = x;
    $("row").value = y;
    text("cell-heading", `Column ${x}, row ${y}${cell.width === 0 ? " / continuation" : ""}`);
    const codepoints = Array.from(cell.char, (char) => `U+${char.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")}`);
    const properties = {
      Grapheme: JSON.stringify(cell.char), Codepoints: codepoints.join(" ") || "(none)", Width: cell.width,
      Foreground: `${cell.fg} -> ${cell.resolved_fg}`, Background: `${cell.bg} -> ${cell.resolved_bg}`,
      Underline: `${cell.underline_style} / ${cell.underline_color} -> ${cell.resolved_underline_color}`,
    };
    for (const flag of ["bold", "dim", "italic", "inverse", "invisible", "strike", "blink"]) properties[flag] = cell.flags.includes(flag);
    const previous = cellAt(frames[index - 1], x, y);
    if (previous) properties["Changed fields (previous frame)"] = Object.keys(cell)
      .filter((key) => json(cell[key]) !== json(previous[key])).join(", ") || "none";
    pairs("cell-properties", properties, { Foreground: cell.resolved_fg, Background: cell.resolved_bg });
    text("cell-json", json({ screen_sequence: frame.sequence, column: x, row: y, ...cell, codepoints,
      lead_column: cell.width === 0 && x > 0 && cellAt(frame, x - 1, y)?.width === 2 ? x - 1 : null }));
    for (const mismatch of mismatches().filter((entry) => entry.x === x && entry.y === y)) {
      $("cell-mismatches").hidden = false;
      $("cell-mismatches").append(element("strong", `${mismatch.property} / stage ${mismatch.stage}`),
        element("p", `Expected: ${mismatch.operator} ${mismatch.expected}`),
        element("p", `Observed: ${mismatch.actual}${mismatch.resolved ? ` (${mismatch.resolved})` : ""}`));
    }
  }

  function attachment(file) {
    if (!downloads.has(file.name)) {
      const bytes = Uint8Array.from(atob(file.data), (char) => char.charCodeAt(0));
      const mime = file.name.endsWith(".svg") ? "image/svg+xml" : file.name.endsWith(".json") ? "application/json" : "text/plain";
      downloads.set(file.name, { bytes, mime, url: URL.createObjectURL(new Blob([bytes], { type: mime })) });
    }
    return downloads.get(file.name);
  }
  for (const file of attachments) {
    const row = element("div", "", "attachment");
    const preview = element("button", `${file.name} (${file.bytes.toLocaleString()} B)`);
    preview.dataset.attachment = file.name;
    preview.addEventListener("click", () => {
      const content = attachment(file);
      $("attachment-preview").replaceChildren(element("h2", file.name));
      if (content.mime === "image/svg+xml") {
        const image = element("img");
        image.src = content.url;
        image.alt = file.name;
        $("attachment-preview").append(image);
      } else {
        const limit = 512 * 1024;
        const value = new TextDecoder().decode(content.bytes.subarray(0, limit), { stream: content.bytes.length > limit });
        $("attachment-preview").append(element("pre", value));
        if (content.bytes.length > limit) $("attachment-preview").append(element("p", "Preview limited to 512 KiB. Download contains the complete file.", "muted"));
      }
    });
    const download = element("a", "Download");
    download.href = "#";
    download.download = file.name;
    download.dataset.download = file.name;
    download.addEventListener("click", () => { download.href = attachment(file).url; });
    row.append(preview, download);
    $("evidence-links").append(row);
  }
  for (const file of files.filter((file) => file.status !== "written")) $("evidence-links")
    .append(element("p", `${file.path}: ${file.status} (${file.reason || "not available"})`, "muted"));
  text("recording-note", attachments.some((file) => file.name === "session.cast")
    ? "The complete included session.cast is embedded and downloadable for continuous replay in an asciicast player."
    : `Recording not included: ${details.recording?.reason || "inclusion is opt-in"}. Retained frame inspection does not require a recording.`);

  $("frame-slider").max = Math.max(0, frames.length - 1);
  $("frame-slider").disabled = !frames.length;
  $("failure").disabled = failureIndex === undefined;
  $("screen-image").addEventListener("error", () => {
    showError("Captured SVG could not be displayed");
    $("terminal").hidden = true;
    text("screen-text", frames[index]?.text || "No retained screen");
    $("screen-text").hidden = false;
  });
  $("terminal").addEventListener("click", (event) => {
    const frame = frames[index];
    if (!frame?.svg) return;
    stop();
    const bounds = $("terminal").getBoundingClientRect();
    inspect(Math.floor((event.clientX - bounds.left) * frame.size.cols / bounds.width),
      Math.floor((event.clientY - bounds.top) * frame.size.rows / bounds.height), true);
  });
  $("terminal").addEventListener("keydown", (event) => {
    const movement = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    const frame = frames[index];
    if (!movement || !frame) return;
    event.preventDefault();
    stop();
    inspect(Math.max(0, Math.min(frame.size.cols - 1, (selectedCell?.x || 0) + movement[0])),
      Math.max(0, Math.min(frame.size.rows - 1, (selectedCell?.y || 0) + movement[1])), true);
  });
  $("inspect").addEventListener("click", () => { stop(); inspect($("column").valueAsNumber, $("row").valueAsNumber); });
  $("assertions-only").addEventListener("change", renderActions);
  $("action-filter").addEventListener("input", renderActions);
  $("zoom").addEventListener("change", zoom);
  for (const [id, delta] of [["previous-point", -1], ["next-point", 1]]) $(id).addEventListener("click", () => {
    const visible = visibleOperations();
    const position = visible.indexOf(selectedOperation);
    const next = position < 0 ? (delta < 0 ? visible.length - 1 : 0) : position + delta;
    if (visible[next]) goOperation(visible[next]);
  });
  for (const [id, delta] of [["previous-frame", -1], ["next-frame", 1]]) $(id)
    .addEventListener("click", () => { stop(); showFrame(index + delta); });
  $("frame-slider").addEventListener("input", () => { stop(); showFrame(Number($("frame-slider").value)); });
  $("failure").addEventListener("click", () => {
    stop();
    setTab("errors-tab");
    showFrame(failureIndex, failureOperation, timeline.failure_screen_sequence);
  });
  $("play").addEventListener("click", () => {
    if (timer) { stop(); return; }
    const nextPoint = visibleOperations().filter((op) => op.is_assertion || op.result !== "ok")
      .find((op) => (frameIndex.get(op.screen_at_return) ?? -1) > index);
    const target = nextPoint ? frameIndex.get(nextPoint.screen_at_return) : frames.length - 1;
    text("play", "Pause");
    timer = setInterval(() => {
      showFrame(Math.min(index + 1, target));
      if (index >= target) { stop(); if (nextPoint) goOperation(nextPoint); }
    }, 400);
  });
  document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
  renderActions();
  const linkedSequence = /^#screen-(\d+)$/.exec(location.hash);
  if (linkedSequence && frameIndex.has(Number(linkedSequence[1]))) showFrame(frameIndex.get(Number(linkedSequence[1])));
  else if (failureIndex !== undefined) $("failure").click();
  else showFrame(undefined, null, timeline.failure_screen_sequence);
})();
