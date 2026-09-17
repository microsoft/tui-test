<script lang="ts">
  import type { TraceFrame, Geometry, CellMismatch } from "../types.js";
  import { cellAt } from "../model.js";
  let { frame, geometry, mismatches, cell }: {
    frame: TraceFrame; geometry: Geometry; mismatches: readonly CellMismatch[]; cell?: { x: number; y: number };
  } = $props();
  const locations = $derived([...new Set(mismatches.filter((entry) => cellAt(frame, entry.x, entry.y)).map((entry) => `${entry.x},${entry.y}`))]);
</script>

<svg id="overlay" aria-hidden="true" viewBox={frame.viewBox}>
  {#each locations as location (location)}
    {@const [x, y] = location.split(",").map(Number)}
    <rect class="mismatch-cell" x={geometry.grid_x + x * geometry.cell_width} y={geometry.grid_y + y * geometry.cell_height}
      width={geometry.cell_width} height={geometry.cell_height} />
  {/each}
  {#if cell}
    <rect class="selected-cell" x={geometry.grid_x + cell.x * geometry.cell_width} y={geometry.grid_y + cell.y * geometry.cell_height}
      width={geometry.cell_width} height={geometry.cell_height} />
  {/if}
</svg>

<style>
  svg { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .selected-cell { fill: #6ca8ff; fill-opacity: .16; stroke: #b6d6ff; stroke-width: 1.2; }
  .mismatch-cell { fill: #ff5369; fill-opacity: .26; }
</style>
