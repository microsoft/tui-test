<script lang="ts">
  import type { TraceFrame, FrameCell, CellInspection } from "../types.js";
  import { json } from "../format.js";
  import PropertyList from "./PropertyList.svelte";
  let { inspection, frame, column, row, oncoordinate, oninspect }: {
    inspection: CellInspection; frame?: TraceFrame; column: string; row: string;
    oncoordinate: (axis: "column" | "row", value: string) => void; oninspect: () => void;
  } = $props();
  const display = $derived.by(() => {
    const properties: Record<string, string | number | boolean> = {};
    if (!inspection) return { heading: "Cell metadata is unavailable at these coordinates.", properties, evidence: "" };
    const { cell, x, y, previous, leadColumn } = inspection;
    const codepoints = Array.from(cell.char, (char) => `U+${char.codePointAt(0)?.toString(16).toUpperCase().padStart(4, "0")}`);
    Object.assign(properties, {
      Grapheme: JSON.stringify(cell.char), Codepoints: codepoints.join(" ") || "(none)", Width: cell.width,
      Foreground: `${cell.fg} -> ${cell.resolved_fg}`, Background: `${cell.bg} -> ${cell.resolved_bg}`,
      Underline: `${cell.underline_style} / ${cell.underline_color} -> ${cell.resolved_underline_color}`,
      Hyperlink: cell.link || "none", "Link ID": cell.link_id || "none",
    });
    for (const flag of ["bold", "dim", "italic", "inverse", "invisible", "strike", "blink"]) properties[flag] = cell.flags.includes(flag);
    if (previous) {
      const keys = [...new Set([...Object.keys(previous), ...Object.keys(cell)])] as (keyof FrameCell)[];
      properties["Changed fields (previous frame)"] = keys.filter((key) => json(cell[key]) !== json(previous[key])).join(", ") || "none";
    }
    return {
      heading: `Column ${x}, row ${y}${cell.width === 0 ? " / continuation" : ""}`, properties,
      evidence: json({ screen_sequence: inspection.frame.sequence, column: x, row: y, ...cell, codepoints, lead_column: leadColumn ?? null }),
    };
  });
</script>

<div class="cell-layout">
  <div>
    <div class="coordinates toolbar">
      <label>Column <input id="column" type="number" min="0" max={frame ? frame.columns - 1 : 0} value={column}
        oninput={(event) => oncoordinate("column", event.currentTarget.value)} /></label>
      <label>Row <input id="row" type="number" min="0" max={frame ? frame.rows - 1 : 0} value={row}
        oninput={(event) => oncoordinate("row", event.currentTarget.value)} /></label>
      <button id="inspect" disabled={!frame?.grid.length} onclick={oninspect}>Inspect</button>
      <span id="cell-heading" aria-live="polite">{display.heading}</span>
    </div>
    <p class="muted">Zero-based viewport coordinates. Empty graphemes mark wide-character continuation cells.</p>
    <PropertyList id="cell-properties" values={display.properties}
      colors={{ Foreground: inspection?.cell.resolved_fg ?? "", Background: inspection?.cell.resolved_bg ?? "" }} />
  </div>
  <div>
    <div id="cell-mismatches" class="cell-mismatches" hidden={!inspection?.mismatches.length}>
      {#each inspection?.mismatches ?? [] as entry}
        <strong>{entry.property} / stage {entry.stage}</strong>
        <p>Expected: {entry.operator} {entry.expected}</p><p>Observed: {entry.actual}{entry.resolved ? ` (${entry.resolved})` : ""}</p>
      {/each}
    </div>
    <details><summary>Raw cell metadata</summary><pre id="cell-json">{display.evidence}</pre></details>
  </div>
</div>

<style>
  .cell-layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 24px; }
  .coordinates input { width: 55px; }
  .coordinates label { font-size: 12px; }
  .cell-mismatches { border-left: 3px solid var(--red); background: var(--error-bg); padding: 8px 12px; }
  .cell-mismatches p { font: 12px/1.5 Consolas, monospace; }
  @media (max-width: 600px) { .cell-layout { grid-template-columns: 1fr; } }
</style>
