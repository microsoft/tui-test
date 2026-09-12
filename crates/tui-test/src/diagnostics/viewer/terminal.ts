import { button, get, input, showError, text, time } from "./dom.js";
import { cellAt, type TraceModel } from "./model.js";
import type { CellInspection, Frame, ImageResult } from "./types.js";

export class TerminalView {
  private readonly images = new Map<number, ImageResult>();
  private selectedCell?: { x: number; y: number };
  private readonly terminal = get("terminal");
  private readonly viewport = get("viewport");
  private readonly overlay = get("overlay", SVGSVGElement);
  private readonly screen = get("screen-image", HTMLImageElement);
  private readonly zoomControl = get("zoom", HTMLSelectElement);
  constructor(
    private readonly model: TraceModel,
    private readonly onInspect: (cell: CellInspection | undefined, activate: boolean) => void,
    private readonly pause: () => void,
  ) {
    this.screen.addEventListener("error", () => {
      showError("Captured SVG could not be displayed");
      this.terminal.hidden = true;
      text("screen-text", model.selection.frame?.text || "No retained screen");
      get("screen-text").hidden = false;
    });
    this.terminal.addEventListener("click", (event) => {
      const frame = model.selection.frame;
      if (!frame) return;
      pause();
      const bounds = this.terminal.getBoundingClientRect();
      this.inspect(Math.floor((event.clientX - bounds.left) * frame.size.cols / bounds.width),
        Math.floor((event.clientY - bounds.top) * frame.size.rows / bounds.height), true);
    });
    this.terminal.addEventListener("keydown", (event) => {
      const moves: Record<string, readonly [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
      const movement = moves[event.key];
      const frame = model.selection.frame;
      if (!movement || !frame) return;
      event.preventDefault();
      pause();
      this.inspect(Math.max(0, Math.min(frame.size.cols - 1, (this.selectedCell?.x || 0) + movement[0])),
        Math.max(0, Math.min(frame.size.rows - 1, (this.selectedCell?.y || 0) + movement[1])), true);
    });
    button("inspect").addEventListener("click", () => { pause(); this.inspect(input("column").valueAsNumber, input("row").valueAsNumber); });
    this.zoomControl.addEventListener("change", () => this.zoom());
    new ResizeObserver(() => this.zoom()).observe(this.viewport);
  }
  image(frame: Frame): ImageResult {
    if (!frame.svg) return {};
    const cached = this.images.get(frame.sequence);
    if (cached) return cached;
    // The downloadable SVG remains unchanged; only the viewer crops its chrome.
    const document = new DOMParser().parseFromString(frame.svg, "image/svg+xml");
    let result: ImageResult;
    if (document.querySelector("parsererror")) {
      result = { error: "Captured SVG could not be rendered. Terminal text and cell metadata are still available." };
    } else {
      const root = document.documentElement;
      root.setAttribute("viewBox", this.viewBox(frame));
      root.setAttribute("width", String(frame.size.cols * this.model.data.timeline.geometry.cell_width));
      root.setAttribute("height", String(frame.size.rows * this.model.data.timeline.geometry.cell_height));
      result = { url: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(root))}` };
    }
    this.images.set(frame.sequence, result);
    return result;
  }
  private viewBox(frame: Frame) {
    const geometry = this.model.data.timeline.geometry;
    return `${geometry.grid_x} ${geometry.grid_y} ${frame.size.cols * geometry.cell_width} ${frame.size.rows * geometry.cell_height}`;
  }
  render() {
    const { frame, index, operation, missingSequence, isFailure, sharesFailureFrame } = this.model.selection;
    const screenshot = frame ? this.image(frame) : {};
    this.selectedCell = undefined;
    this.terminal.hidden = true;
    get("screen-text").hidden = true;
    get("frame-notice").hidden = !frame?.omission && !screenshot.error;
    text("frame-heading", operation?.name || "Terminal snapshot");
    text("frame-meta", frame ? `Screen ${frame.sequence}${sharesFailureFrame ? (isFailure ? " / failure" : " / also used by failure") : ""} / ` +
      `${frame.size.cols} x ${frame.size.rows} / ${time(frame.first_seen_ms)} - ${time(frame.last_seen_ms)} / ${frame.changes.join(", ")}` :
      `Screen ${missingSequence ?? "unknown"} was not retained. No substitute frame is shown.`);
    text("frame-count", frame ? `${index + 1} / ${this.model.frames.length}` : "Unavailable");
    button("inspect").disabled = !frame?.grid.length;
    if (frame) {
      input("column").max = String(frame.size.cols - 1);
      input("row").max = String(frame.size.rows - 1);
      text("frame-notice", frame.omission || screenshot.error || "");
      if (screenshot.url) {
        this.overlay.setAttribute("viewBox", this.viewBox(frame));
        this.screen.src = screenshot.url;
        this.terminal.hidden = false;
        this.zoom();
      } else {
        text("screen-text", frame.text);
        get("screen-text").hidden = false;
      }
      const mismatch = this.model.mismatches().find((entry) => cellAt(frame, entry.x, entry.y));
      this.inspect(mismatch?.x ?? Math.min(frame.cursor.column, frame.size.cols - 1),
        mismatch?.y ?? Math.min(frame.cursor.row, frame.size.rows - 1));
    } else {
      text("screen-text", "This checkpoint's screen was evicted or could not be captured. Select another action or jump to failure.");
      get("screen-text").hidden = false;
      this.inspect(-1, -1);
    }
  }
  private zoom() {
    const frame = this.model.selection.frame;
    if (!frame) return;
    const geometry = this.model.data.timeline.geometry;
    const width = frame.size.cols * geometry.cell_width;
    const height = frame.size.rows * geometry.cell_height;
    const padding = getComputedStyle(this.viewport);
    const availableWidth = this.viewport.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight);
    const availableHeight = this.viewport.clientHeight - parseFloat(padding.paddingTop) - parseFloat(padding.paddingBottom);
    const fit = this.zoomControl.value === "fit";
    const scale = fit ? Math.min(1, Math.max(1, availableWidth) / width, Math.max(1, availableHeight) / height) : Number(this.zoomControl.value);
    this.viewport.classList.toggle("fit", fit);
    this.terminal.style.width = `${width * scale}px`;
  }
  private drawOverlay() {
    this.overlay.replaceChildren();
    const frame = this.model.selection.frame;
    if (!frame?.svg || this.image(frame).error) return;
    const geometry = this.model.data.timeline.geometry;
    const rect = (x: number, y: number, className: string) => {
      const node = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      for (const [name, value] of Object.entries({
        x: geometry.grid_x + x * geometry.cell_width, y: geometry.grid_y + y * geometry.cell_height,
        width: geometry.cell_width, height: geometry.cell_height, class: className,
      })) node.setAttribute(name, String(value));
      this.overlay.append(node);
    };
    const locations = new Set(this.model.mismatches().filter((entry) => cellAt(frame, entry.x, entry.y)).map((entry) => `${entry.x},${entry.y}`));
    for (const location of locations) {
      const [x, y] = location.split(",").map(Number);
      rect(x, y, "mismatch-cell");
    }
    if (this.selectedCell) rect(this.selectedCell.x, this.selectedCell.y, "selected-cell");
  }
  private inspect(x: number, y: number, activate = false) {
    const { frame, index } = this.model.selection;
    const cell = Number.isInteger(x) && Number.isInteger(y) ? cellAt(frame, x, y) : undefined;
    this.selectedCell = cell ? { x, y } : undefined;
    this.drawOverlay();
    const inspection = cell && frame ? {
      frame, x, y, cell, previous: cellAt(this.model.frames[index - 1], x, y),
      leadColumn: cell.width === 0 && x > 0 && cellAt(frame, x - 1, y)?.width === 2 ? x - 1 : undefined,
      mismatches: this.model.mismatches().filter((entry) => entry.x === x && entry.y === y),
    } : undefined;
    this.onInspect(inspection, activate);
  }
}
