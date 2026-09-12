import { button, element, get, input, time } from "./dom.js";
import { describeExpectation } from "./expectation.js";
import type { TraceModel } from "./model.js";
import type { Frame, ImageResult, Operation } from "./types.js";

export class ActionsPanel {
  constructor(private readonly model: TraceModel, private readonly select: (operation: Operation) => void) {
    input("assertions-only").addEventListener("change", () => this.render());
    input("action-filter").addEventListener("input", () => this.render());
    for (const [id, delta] of [["previous-point", -1], ["next-point", 1]] as const) {
      button(id).addEventListener("click", () => {
        const actions = this.visible();
        const position = actions.findIndex((op) => op === model.selection.operation);
        const next = position < 0 ? (delta < 0 ? actions.length - 1 : 0) : position + delta;
        if (actions[next]) select(actions[next]);
      });
    }
    this.render();
  }
  visible() {
    const filter = input("action-filter").value.toLowerCase();
    return this.model.operations.filter((op) =>
      (!input("assertions-only").checked || op.is_assertion || op.result !== "ok") &&
      `${op.name} ${op.safe_summary} ${describeExpectation(op.expectation) || ""}`.toLowerCase().includes(filter));
  }
  private render() {
    const container = get("points");
    container.replaceChildren();
    for (const op of this.visible()) {
      const item = element("button", "", `point${op.result !== "ok" ? " failed" : ""}`);
      item.dataset.operation = String(op.sequence);
      const expectation = describeExpectation(op.expectation) || op.safe_summary;
      item.title = `${expectation} / screen ${op.screen_at_return}`;
      const label = element("span");
      label.append(element("span", `${op.sequence}. ${op.name}`, "name"), element("span", expectation, "description"));
      item.append(element("span", "", "status"), label, element("span",
        `${op.result === "ok" ? "PASS" : "FAIL"}\n${time(op.ended_ms - op.started_ms)}` +
        (this.model.frameIndex.has(op.screen_at_return) ? "" : "\nnot retained"), "result"));
      item.addEventListener("click", () => this.select(op));
      container.append(item);
    }
    if (!container.childElementCount) container.append(element("p", "No matching actions.", "muted"));
    this.updateSelection();
  }
  updateSelection() {
    const actions = this.visible();
    const position = actions.findIndex((op) => op === this.model.selection.operation);
    button("previous-point").disabled = !actions.length || position === 0;
    button("next-point").disabled = !actions.length || position === actions.length - 1;
    for (const item of get("points").querySelectorAll<HTMLButtonElement>("[data-operation]")) {
      item.setAttribute("aria-current", String(Number(item.dataset.operation) === this.model.selection.operation?.sequence));
    }
  }
}

export class TimelineView {
  constructor(
    private readonly model: TraceModel,
    image: (frame: Frame) => ImageResult,
    selectAction: (operation: Operation) => void,
    selectFrame: (index: number) => void,
  ) {
    for (let tick = 0; tick <= 5; tick++) get("time-scale").append(element("span", time(Math.round(model.duration * tick / 5))));
    for (const op of model.operations) {
      const bar = element("button", "", `action-bar${op.result !== "ok" ? " failed" : ""}`);
      bar.style.left = `${Math.min(99.5, op.started_ms / Math.max(1, model.duration) * 100)}%`;
      bar.style.width = `${Math.max(.4, (op.ended_ms - op.started_ms) / Math.max(1, model.duration) * 100)}%`;
      bar.dataset.operation = String(op.sequence);
      bar.title = `${describeExpectation(op.expectation) || op.name}: ${time(op.ended_ms - op.started_ms)} (${op.result})`;
      bar.setAttribute("aria-label", bar.title);
      bar.addEventListener("click", () => selectAction(op));
      get("action-timeline").append(bar);
    }
    model.frames.forEach((frame, index) => {
      const item = element("button", "", "thumbnail");
      item.dataset.frame = String(index);
      item.setAttribute("aria-label", `Screen ${frame.sequence} at ${time(frame.first_seen_ms)}`);
      const screenshot = image(frame);
      if (screenshot.url) {
        const thumbnail = element("img");
        thumbnail.src = screenshot.url;
        thumbnail.alt = "";
        item.append(thumbnail);
      }
      item.append(element("span", `#${frame.sequence} / ${time(frame.first_seen_ms)}`));
      item.addEventListener("click", () => selectFrame(index));
      get("filmstrip").append(item);
    });
  }
  updateSelection() {
    for (const item of get("action-timeline").querySelectorAll<HTMLButtonElement>("button")) {
      item.setAttribute("aria-current", String(Number(item.dataset.operation) === this.model.selection.operation?.sequence));
    }
    for (const item of get("filmstrip").querySelectorAll<HTMLButtonElement>("button")) {
      item.setAttribute("aria-current", String(Number(item.dataset.frame) === this.model.selection.index));
    }
  }
}
