(() => {
  "use strict";
  const $ = (id) => document.getElementById(id);
  const text = (id, value) => { $(id).textContent = value; };
  const json = (value) => JSON.stringify(value, null, 2);
  const showError = (message) => {
    text("report-error", `Report viewer error: ${message}. The original evidence remains in failure.json and current.svg.`);
    $("report-error").hidden = false;
  };
  window.addEventListener("error", (event) => showError(event.message));
  window.addEventListener("unhandledrejection", (event) => showError(String(event.reason)));

  const { details, timeline, files, errors } = JSON.parse($("report-data").textContent);
  const frames = timeline.frames;
  const operations = details.recent_operations || [];
  const frameIndex = new Map(frames.map((frame, index) => [frame.sequence, index]));
  const geometry = timeline.geometry;
  const failureIndex = frameIndex.get(timeline.failure_screen_sequence);
  const failureOperation = operations.findLast((op) => op.result !== "ok" &&
    op.screen_at_return === timeline.failure_screen_sequence) || null;
  let index = -1;
  let selectedOperation = null;
  let selectedCell = null;
  let timer = null;
  let imageSize = null;
  const pointOperations = () => $("all-operations").checked ? operations :
    operations.filter((op) => op.is_assertion || op.result !== "ok");

  text("operation", details.operation.name);
  text("summary", details.summary);
  document.title = `tui-test: ${details.operation.name} failed`;
  for (const [value, failed] of [
    [details.reason, true], [`${details.operation.elapsed_ms} ms`, false],
    [details.runtime?.backend || "backend unavailable", false],
    [`schema ${details.schema_version}`, false],
  ]) {
    const badge = document.createElement("span");
    badge.className = `badge${failed ? " fail" : ""}`;
    badge.textContent = value;
    $("badges").append(badge);
  }
  const history = details.terminal?.screen_history;
  text("retention", `Bounded observations, not every PTY write. ${frames.length} frames retained; ` +
    `${history?.dropped_screen_count || 0} sampled screens evicted; ` +
    `${history?.dropped_checkpoint_count || 0} checkpoints evicted or omitted. ` +
    "Missing frames are never interpolated. Playback steps retained frames at 400 ms per frame. " +
    "Passing checkpoints show the screen at operation return; failure shows the pinned evaluation.");
  text("diagnosis", json({
    comparison: details.comparison, locator: details.locator,
    evaluation_transitions: details.evaluation_transitions,
  }));
  text("runtime", json({
    runtime: details.runtime, process: details.process, context: details.context,
    recording: details.recording, truncated: details.truncated, signature: details.signature, files, errors,
  }));
  const allowedFiles = new Set(["current.svg", "current.txt", "timeline.json", "session.cast"]);
  for (const path of ["failure.md", "failure.json",
    ...files.filter((file) => file.status === "written" && allowedFiles.has(file.path)).map((file) => file.path)]) {
    const link = document.createElement("a");
    link.href = path;
    link.textContent = path;
    $("evidence-links").append(link);
  }
  text("recording-note", files.some((file) => file.path === "session.cast" && file.status === "written")
    ? "session.cast is available for continuous replay in an asciicast player. This inspector uses original emulator snapshots, not a second emulator's reconstruction."
    : `No continuous replay is bundled. ${details.recording?.reason || "Recording inclusion is opt-in."}`);
  for (const hint of details.hints || []) {
    const item = document.createElement("li");
    item.textContent = `${hint.code}: ${hint.message}`;
    $("hints").append(item);
  }
  $("frame-slider").max = Math.max(0, frames.length - 1);
  $("frame-slider").disabled = frames.length === 0;
  $("failure").disabled = failureIndex === undefined;

  function stop() {
    clearInterval(timer);
    timer = null;
    text("play", "Play to next point");
  }

  function renderPoints() {
    $("points").replaceChildren();
    for (const op of pointOperations()) {
      const button = document.createElement("button");
      button.className = `point${op.result !== "ok" ? " failed" : ""}`;
      button.dataset.operation = op.sequence;
      button.setAttribute("aria-current", String(op === selectedOperation));
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = `${op.sequence}. ${op.name}`;
      const status = document.createElement("span");
      status.className = "result";
      status.textContent = `${op.result === "ok" ? "PASS" : "FAIL: " + op.result} / ${op.ended_ms} ms / screen ${op.screen_at_return}` +
        (frameIndex.has(op.screen_at_return) ? "" : " (not retained)");
      button.append(name, status);
      button.addEventListener("click", () => goOperation(op));
      $("points").append(button);
    }
    const points = pointOperations();
    const position = points.indexOf(selectedOperation);
    $("previous-point").disabled = !points.length || position === 0;
    $("next-point").disabled = !points.length || position === points.length - 1;
  }

  function goOperation(op, before = false) {
    stop();
    const sequence = before ? op.screen_before : op.screen_at_return;
    showFrame(frameIndex.get(sequence), op, before ? "Before operation" : "At return", sequence);
  }

  function showFrame(next, op = null, phase = "", missingSequence = null) {
    index = next === undefined ? -1 : next;
    selectedOperation = op;
    selectedCell = null;
    imageSize = null;
    $("overlay").replaceChildren();
    $("terminal").hidden = true;
    $("screen-text").hidden = true;
    $("frame-notice").hidden = true;
    $("cell-properties").replaceChildren();
    $("cell-mismatches").hidden = true;
    text("cell-heading", "Select a cell.");
    text("cell-json", "");
    $("before").disabled = !op;
    $("after").disabled = !op;
    renderPoints();
    const frame = frames[index];
    text("frame-heading", op ? `${op.name} / ${phase}` : "Observed frame");
    const failureLabel = index === failureIndex
      ? (op && op !== failureOperation ? " / also used by failure" : " / PINNED FAILURE") : "";
    text("frame-meta", frame ? `Screen ${frame.sequence}${failureLabel} / ` +
      `${frame.first_seen_ms}-${frame.last_seen_ms} ms / ${frame.size.cols} x ${frame.size.rows} / ${frame.changes.join(", ")}` :
      `Screen ${missingSequence ?? "unknown"} was not retained. No substitute frame is shown.`);
    text("frame-count", frame ? `Frame ${index + 1} of ${frames.length}` : "Frame unavailable");
    $("previous-frame").disabled = index <= 0;
    $("next-frame").disabled = frames.length === 0 || index >= frames.length - 1;
    $("play").disabled = index < 0 || index >= frames.length - 1;
    $("inspect").disabled = !frame?.grid.length;
    if (frame) {
      $("frame-slider").value = index;
      $("column").max = frame.size.cols - 1;
      $("row").max = frame.size.rows - 1;
      const { cells, grid, svg, ...metadata } = frame;
      text("frame-details", json({ operation: op, phase, frame: metadata,
        operations_at_this_screen: operations.filter((entry) => entry.screen_at_return === frame.sequence) }));
      if (frame.omission) {
        text("frame-notice", frame.omission);
        $("frame-notice").hidden = false;
      }
      if (frame.svg) {
        // Read geometry from the inert SVG document, never insert terminal markup into the page.
        const svgDocument = new DOMParser().parseFromString(frame.svg, "image/svg+xml");
        const viewBox = svgDocument.documentElement.getAttribute("viewBox");
        const values = viewBox?.split(/\s+/).map(Number);
        if (!values || values.length !== 4 || !values.every(Number.isFinite)) {
          throw new Error("Captured SVG has no valid viewBox");
        }
        imageSize = { width: values[2], height: values[3] };
        $("overlay").setAttribute("viewBox", viewBox);
        $("screen-image").src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(frame.svg)}`;
        $("terminal").hidden = false;
      } else {
        text("screen-text", frame.text);
        $("screen-text").hidden = false;
      }
      const mismatch = mismatches().find((entry) => entry.x >= 0 && entry.x < frame.size.cols &&
        entry.y >= 0 && entry.y < frame.size.rows);
      inspect(mismatch?.x ?? Math.min(frame.cursor.column, frame.size.cols - 1),
        mismatch?.y ?? Math.min(frame.cursor.row, frame.size.rows - 1));
    } else {
      text("frame-details", json({ operation: op, missing_screen_sequence: missingSequence }));
      text("screen-text", "This checkpoint's screen was evicted or could not be captured. Select another point or jump to failure.");
      $("screen-text").hidden = false;
    }
  }

  function mismatches() {
    if (index !== failureIndex || !details.locator ||
      (selectedOperation && selectedOperation !== failureOperation)) return [];
    return (details.locator.stages || []).flatMap((stage) => (stage.mismatches || []).map((mismatch) => ({
      ...mismatch, stage: stage.stage_index, x: mismatch.location.column,
      y: mismatch.location.row - details.locator.viewport_origin_y,
    })));
  }

  function cellAt(frame, x, y) {
    const id = frame?.grid[y]?.[x];
    return id === undefined ? undefined : frame.cells[id];
  }

  function rect(x, y, className) {
    const element = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    for (const [name, value] of Object.entries({
      x: geometry.grid_x + x * geometry.cell_width, y: geometry.grid_y + y * geometry.cell_height,
      width: geometry.cell_width, height: geometry.cell_height, class: className,
    })) element.setAttribute(name, value);
    return element;
  }

  function inspect(x, y) {
    const frame = frames[index];
    const cell = Number.isInteger(x) && Number.isInteger(y) ? cellAt(frame, x, y) : undefined;
    $("cell-properties").replaceChildren();
    $("cell-mismatches").hidden = true;
    $("overlay").replaceChildren();
    text("cell-json", "");
    if (!cell) {
      selectedCell = null;
      text("cell-heading", "Cell metadata is unavailable at these coordinates.");
      return;
    }
    selectedCell = { x, y };
    $("column").value = x;
    $("row").value = y;
    text("cell-heading", `Column ${x}, row ${y}${cell.width === 0 ? " / continuation cell" : ""}`);
    const codepoints = Array.from(cell.char, (char) => `U+${char.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")}`);
    const properties = {
      Grapheme: JSON.stringify(cell.char), Codepoints: codepoints.join(" ") || "(none)",
      Width: cell.width, Foreground: `${cell.fg} -> ${cell.resolved_fg}`,
      Background: `${cell.bg} -> ${cell.resolved_bg}`, Underline: cell.underline_style,
      "Underline color": `${cell.underline_color} -> ${cell.resolved_underline_color}`,
    };
    for (const flag of ["bold", "dim", "italic", "inverse", "invisible", "strike", "blink"]) {
      properties[flag] = cell.flags.includes(flag);
    }
    const previous = cellAt(frames[index - 1], x, y);
    if (previous) {
      properties["Changed fields (previous retained frame)"] = Object.keys(cell)
        .filter((key) => json(cell[key]) !== json(previous[key])).join(", ") || "none";
    }
    for (const [name, value] of Object.entries(properties)) {
      const label = document.createElement("dt");
      label.textContent = name;
      const content = document.createElement("dd");
      content.textContent = String(value);
      const color = name === "Foreground" ? cell.resolved_fg : name === "Background" ? cell.resolved_bg : null;
      if (color && /^#[0-9a-f]{6}$/i.test(color)) {
        const swatch = document.createElement("span");
        swatch.className = "swatch";
        swatch.style.backgroundColor = color;
        content.prepend(swatch);
      }
      $("cell-properties").append(label, content);
    }
    text("cell-json", json({ screen_sequence: frame.sequence, column: x, row: y, ...cell, codepoints,
      lead_column: cell.width === 0 && x > 0 && cellAt(frame, x - 1, y)?.width === 2 ? x - 1 : null }));
    const allMismatches = mismatches();
    const localMismatches = allMismatches.filter((entry) => entry.x === x && entry.y === y);
    if (localMismatches.length) {
      text("cell-mismatches", json(localMismatches));
      $("cell-mismatches").hidden = false;
    }
    if (imageSize) {
      for (const entry of allMismatches) {
        if (entry.x >= 0 && entry.x < frame.size.cols && entry.y >= 0 && entry.y < frame.size.rows) {
          $("overlay").append(rect(entry.x, entry.y, "mismatch-cell"));
        }
      }
      $("overlay").append(rect(x, y, "selected-cell"));
    }
  }

  $("screen-image").addEventListener("error", () => {
    showError("Captured SVG could not be displayed");
    $("terminal").hidden = true;
    text("screen-text", frames[index]?.text || "No retained screen");
    $("screen-text").hidden = false;
  });
  $("terminal").addEventListener("click", (event) => {
    if (!imageSize) return;
    stop();
    const bounds = $("terminal").getBoundingClientRect();
    const x = Math.floor(((event.clientX - bounds.left) * imageSize.width / bounds.width - geometry.grid_x) / geometry.cell_width);
    const y = Math.floor(((event.clientY - bounds.top) * imageSize.height / bounds.height - geometry.grid_y) / geometry.cell_height);
    inspect(x, y);
  });
  $("terminal").addEventListener("keydown", (event) => {
    const movement = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!movement || !frames[index]) return;
    event.preventDefault();
    stop();
    const frame = frames[index];
    inspect(Math.max(0, Math.min(frame.size.cols - 1, (selectedCell?.x || 0) + movement[0])),
      Math.max(0, Math.min(frame.size.rows - 1, (selectedCell?.y || 0) + movement[1])));
  });
  $("inspect").addEventListener("click", () => {
    stop();
    inspect($("column").valueAsNumber, $("row").valueAsNumber);
  });
  $("all-operations").addEventListener("change", renderPoints);
  $("before").addEventListener("click", () => goOperation(selectedOperation, true));
  $("after").addEventListener("click", () => goOperation(selectedOperation));
  const goPoint = (delta) => {
    const points = pointOperations();
    const position = points.indexOf(selectedOperation);
    const next = position < 0 ? (delta < 0 ? points.length - 1 : 0) : position + delta;
    if (points[next]) goOperation(points[next]);
  };
  $("previous-point").addEventListener("click", () => goPoint(-1));
  $("next-point").addEventListener("click", () => goPoint(1));
  for (const [id, delta] of [["previous-frame", -1], ["next-frame", 1]]) {
    $(id).addEventListener("click", () => { stop(); showFrame(index + delta); });
  }
  $("frame-slider").addEventListener("input", () => { stop(); showFrame(Number($("frame-slider").value)); });
  $("failure").addEventListener("click", () => {
    stop();
    showFrame(failureIndex, failureOperation, "Pinned failure");
  });
  $("play").addEventListener("click", () => {
    if (timer) { stop(); return; }
    const nextPoint = pointOperations().find((op) => {
      const target = frameIndex.get(op.screen_at_return);
      return target !== undefined && target > index;
    });
    const target = nextPoint ? frameIndex.get(nextPoint.screen_at_return) : frames.length - 1;
    text("play", "Pause");
    timer = setInterval(() => {
      showFrame(Math.min(index + 1, target));
      if (index >= target) {
        stop();
        if (nextPoint) goOperation(nextPoint);
      }
    }, 400);
  });
  document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
  const linkedSequence = /^#screen-(\d+)$/.exec(location.hash);
  if (linkedSequence && frameIndex.has(Number(linkedSequence[1]))) {
    showFrame(frameIndex.get(Number(linkedSequence[1])));
  } else {
    $("failure").click();
    if (failureIndex === undefined) {
      showFrame(undefined, null, "", timeline.failure_screen_sequence);
    }
  }
})();
