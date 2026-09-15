<script lang="ts">
  import type { TraceFrame, Geometry, CellMismatch, ViewerState } from "../types.js";
  import CellOverlay from "./CellOverlay.svelte";
  let { frame, url, zoom, geometry, mismatches, cell, oninspect, onmove, onimageerror }: {
    frame?: TraceFrame; url?: string; zoom: ViewerState["zoom"]; geometry: Geometry;
    mismatches: readonly CellMismatch[]; cell?: { x: number; y: number };
    oninspect: (x: number, y: number) => void; onmove: (dx: number, dy: number) => void; onimageerror: (index: number) => void;
  } = $props();
  const width = $derived(!frame ? undefined : zoom === "fit"
    ? `min(${frame.width}px, 100cqw, ${100 * frame.width / frame.height}cqh)`
    : `${frame.width * Number(zoom)}px`);

  function inspect(event: MouseEvent, terminal: HTMLButtonElement) {
    if (!frame) return;
    if (event.detail === 0) {
      oninspect(cell?.x ?? Math.min(frame.cursor.x, frame.columns - 1), cell?.y ?? Math.min(frame.cursor.y, frame.rows - 1));
      return;
    }
    const bounds = terminal.getBoundingClientRect();
    oninspect(Math.floor((event.clientX - bounds.left) * frame.columns / bounds.width),
      Math.floor((event.clientY - bounds.top) * frame.rows / bounds.height));
  }

  function move(event: KeyboardEvent) {
    const moves: Record<string, readonly [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    const delta = moves[event.key];
    if (delta) { event.preventDefault(); onmove(...delta); }
  }
</script>

<div class="viewport" class:fit={zoom === "fit"} id="viewport">
  <button id="terminal" type="button" style:width={width} hidden={!url}
    aria-label="Terminal cell inspector. Click a cell or use arrow keys."
    onclick={(event) => inspect(event, event.currentTarget)} onkeydown={move}>
    {#if url && frame}
      <img id="screen-image" src={url} alt="Captured terminal screen" draggable="false" onerror={() => onimageerror(frame.index)} />
      <CellOverlay {frame} {geometry} {mismatches} {cell} />
    {/if}
  </button>
  <pre id="screen-text" hidden={!!url}>{frame?.text ?? "This checkpoint's screen was evicted or could not be captured. Select another action or jump to failure."}</pre>
</div>

<style>
  .viewport { container-type: size; display: flex; align-items: flex-start; flex: 1; min-height: 0; overflow: auto; padding: 12px; background: var(--panel); }
  #terminal { position: relative; flex-shrink: 0; margin: 0 auto; padding: 0; border: 0; border-radius: 0; line-height: 0; cursor: crosshair; box-shadow: 0 1px 5px #0003; }
  .fit #terminal { max-width: 100%; }
  #screen-image { width: 100%; height: auto; display: block; }
  #screen-text { padding: 10px; white-space: pre; align-self: stretch; width: 100%; }
  @media (max-width: 600px) { .viewport { padding: 5px; } }
</style>
