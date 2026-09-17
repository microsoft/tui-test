<script lang="ts">
  import type { TraceFrame, TraceAction, TraceModel, ImageResult, Geometry, CellMismatch, ViewerState } from "../types.js";
  import { IMAGE_ERROR } from "../resources.js";
  import ExpectationBanner from "./ExpectationBanner.svelte";
  import TerminalScreen from "./TerminalScreen.svelte";
  let { frame, action, missingScreen, sharesFailure, isFailure, result, actions, frameCount, zoom, playing,
    image, imageFailed, geometry, mismatches, cell, onaction, onframe, onplay, onzoom, oninspect, onmove, onimageerror }: {
    frame?: TraceFrame; action?: TraceAction; missingScreen?: number; sharesFailure: boolean; isFailure: boolean;
    result: TraceModel["result"]; actions: readonly TraceAction[]; frameCount: number; zoom: ViewerState["zoom"]; playing: boolean;
    image?: ImageResult; imageFailed: boolean; geometry: Geometry; mismatches: readonly CellMismatch[]; cell?: { x: number; y: number };
    onaction: (delta: -1 | 1) => void; onframe: (index: number) => void; onplay: () => void;
    onzoom: (zoom: ViewerState["zoom"]) => void; oninspect: (x: number, y: number) => void;
    onmove: (dx: number, dy: number) => void; onimageerror: (index: number) => void;
  } = $props();
  const index = $derived(frame?.index ?? -1);
  const position = $derived(actions.findIndex((item) => item.id === action?.id));
  const imageError = $derived(imageFailed ? IMAGE_ERROR : image?.error);
  const notice = $derived([frame?.omission, imageError].filter(Boolean).join(" "));
  const meta = $derived(frame
    ? `Screen ${frame.sequence}${sharesFailure ? (isFailure ? " / failure" : " / also used by failure") : ""} / ${frame.description}`
    : `Screen ${missingScreen ?? "unknown"} was not retained. No substitute frame is shown.`);

  function changeZoom(event: Event & { currentTarget: HTMLSelectElement }) {
    const value = event.currentTarget.value;
    if (value !== "fit" && value !== "1" && value !== "1.5") throw new Error(`Unknown zoom: ${value}`);
    onzoom(value);
  }
</script>

<section class="viewer" aria-label="Terminal snapshot">
  <div class="viewer-toolbar">
    <strong id="frame-heading">{action?.name ?? "Terminal snapshot"}</strong>
    <div class="toolbar">
      <button id="previous-point" aria-label="Previous action" disabled={!actions.length || position === 0} onclick={() => onaction(-1)}>Prev action</button>
      <button id="next-point" aria-label="Next action" disabled={!actions.length || position === actions.length - 1} onclick={() => onaction(1)}>Next action</button>
      <label class="zoom-label">Zoom <select id="zoom" aria-label="Terminal zoom" value={zoom} onchange={changeZoom}>
        <option value="fit">Fit</option><option value="1">100%</option><option value="1.5">150%</option>
      </select></label>
    </div>
  </div>
  <ExpectationBanner {action} {isFailure} {result} />
  <p id="frame-meta" class="frame-meta" aria-live="polite">{meta}</p>
  <div id="frame-notice" class="notice" hidden={!notice}>{notice}</div>
  <TerminalScreen {frame} url={imageError ? undefined : image?.url} {zoom} {geometry} {mismatches} {cell} {oninspect} {onmove} {onimageerror} />
  <div class="frame-controls">
    <button id="previous-frame" aria-label="Previous retained frame" disabled={index <= 0} onclick={() => onframe(index - 1)}>Prev frame</button>
    <button id="play" disabled={index < 0 || index >= frameCount - 1} onclick={onplay}>{playing ? "Pause" : "Play to next point"}</button>
    <input id="frame-slider" type="range" min="0" max={Math.max(0, frameCount - 1)} value={Math.max(0, index)}
      aria-label="Retained frame" disabled={!frameCount} oninput={(event) => onframe(event.currentTarget.valueAsNumber)} />
    <button id="next-frame" aria-label="Next retained frame" disabled={!frameCount || index >= frameCount - 1} onclick={() => onframe(index + 1)}>Next frame</button>
    <span id="frame-count" class="muted">{frame ? `${index + 1} / ${frameCount}` : "Unavailable"}</span>
  </div>
</section>

<style>
  .viewer { display: flex; flex-direction: column; min-height: 0; min-width: 0; background: var(--bg); }
  .viewer-toolbar { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; justify-content: space-between; padding: 6px 10px; border-bottom: 1px solid var(--border); }
  .viewer-toolbar strong { overflow-wrap: anywhere; min-width: 0; }
  .zoom-label { font-size: 11px; color: var(--muted); }
  .frame-meta { color: var(--muted); font-size: 11px; margin: 0; padding: 4px 10px; }
  .frame-controls { display: flex; align-items: center; gap: 6px; padding: 5px 10px; border-top: 1px solid var(--border); }
  .frame-controls button { white-space: nowrap; font-size: 11px; }
  #frame-slider { flex: 1; min-width: 30px; accent-color: var(--blue); }
  #frame-count { white-space: nowrap; }
  @media (max-width: 850px) { .frame-controls { flex-wrap: wrap; } .viewer-toolbar { align-items: flex-start; } #frame-count { display: none; } }
  @media (max-width: 600px) { .zoom-label { display: none; } }
</style>
