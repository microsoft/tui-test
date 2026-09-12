import { ActionsPanel, TimelineView } from "./actions.js";
import { AttachmentsPanel } from "./attachments.js";
import { CellInspector } from "./cells.js";
import { DetailsPanel } from "./details.js";
import { button, get, input, showError, Tabs, text } from "./dom.js";
import { renderMetadata } from "./metadata.js";
import { TraceModel } from "./model.js";
import { TerminalView } from "./terminal.js";
import type { Operation, ReportData } from "./types.js";

window.addEventListener("error", (event) => showError(event.message));
window.addEventListener("unhandledrejection", (event) => showError(String(event.reason)));

const data: ReportData = JSON.parse(get("report-data").textContent || "");
const model = new TraceModel(data);
const tabs = new Tabs();
const details = new DetailsPanel(data);
const cells = new CellInspector();
let timer: ReturnType<typeof setInterval> | undefined;

function pause() {
  clearInterval(timer);
  timer = undefined;
  text("play", "Play to next point");
}
function selectAction(operation: Operation) {
  pause();
  model.selectAction(operation);
  render();
}
function selectFrame(index: number) {
  pause();
  model.selectFrame(index);
  render();
}
const terminal = new TerminalView(model, (inspection, activate) => {
  cells.render(inspection);
  if (activate) tabs.select("cell-tab");
}, pause);
const actions = new ActionsPanel(model, selectAction);
const timeline = new TimelineView(model, (frame) => terminal.image(frame), selectAction, selectFrame);
renderMetadata(model);
new AttachmentsPanel(data);

function render() {
  actions.updateSelection();
  timeline.updateSelection();
  terminal.render();
  details.render(model.selection);
  const index = model.selection.index;
  input("frame-slider").value = String(Math.max(0, index));
  button("previous-frame").disabled = index <= 0;
  button("next-frame").disabled = !model.frames.length || index >= model.frames.length - 1;
  button("play").disabled = index < 0 || index >= model.frames.length - 1;
}

input("frame-slider").max = String(Math.max(0, model.frames.length - 1));
input("frame-slider").disabled = !model.frames.length;
input("frame-slider").addEventListener("input", () => selectFrame(input("frame-slider").valueAsNumber));
button("previous-frame").addEventListener("click", () => selectFrame(model.selection.index - 1));
button("next-frame").addEventListener("click", () => selectFrame(model.selection.index + 1));
button("failure").disabled = model.failureIndex === undefined;
button("failure").addEventListener("click", () => {
  pause();
  tabs.select("errors-tab");
  model.selectFailure();
  render();
});
button("play").addEventListener("click", () => {
  if (timer) { pause(); return; }
  const next = actions.visible().filter((op) => op.is_assertion || op.result !== "ok")
    .find((op) => (model.frameIndex.get(op.screen_at_return) ?? -1) > model.selection.index);
  const target = next ? model.frameIndex.get(next.screen_at_return)! : model.frames.length - 1;
  text("play", "Pause");
  timer = setInterval(() => {
    model.selectFrame(Math.min(model.selection.index + 1, target));
    render();
    if (model.selection.index >= target) {
      pause();
      if (next) selectAction(next);
    }
  }, 400);
});
document.addEventListener("visibilitychange", () => { if (document.hidden) pause(); });
const linked = /^#screen-(\d+)$/.exec(location.hash);
if (linked && model.frameIndex.has(Number(linked[1]))) model.selectFrame(model.frameIndex.get(Number(linked[1])));
else model.selectFailure();
render();
