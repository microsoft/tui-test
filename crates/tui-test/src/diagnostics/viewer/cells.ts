import { element, get, input, json, pairs, text } from "./dom.js";
import type { CellInspection, FrameCell } from "./types.js";

export class CellInspector {
  render(inspection?: CellInspection) {
    get("cell-properties").replaceChildren();
    get("cell-mismatches").replaceChildren();
    get("cell-mismatches").hidden = true;
    text("cell-json", "");
    if (!inspection) {
      text("cell-heading", "Cell metadata is unavailable at these coordinates.");
      return;
    }
    const { frame, cell, x, y, previous, leadColumn, mismatches } = inspection;
    input("column").value = String(x);
    input("row").value = String(y);
    text("cell-heading", `Column ${x}, row ${y}${cell.width === 0 ? " / continuation" : ""}`);
    const codepoints = Array.from(cell.char, (char) => `U+${char.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}`);
    const properties: Record<string, string | number | boolean> = {
      Grapheme: JSON.stringify(cell.char), Codepoints: codepoints.join(" ") || "(none)", Width: cell.width,
      Foreground: `${cell.fg} -> ${cell.resolved_fg}`, Background: `${cell.bg} -> ${cell.resolved_bg}`,
      Underline: `${cell.underline_style} / ${cell.underline_color} -> ${cell.resolved_underline_color}`,
      Hyperlink: cell.link || "none", "Link ID": cell.link_id || "none",
    };
    for (const flag of ["bold", "dim", "italic", "inverse", "invisible", "strike", "blink"]) properties[flag] = cell.flags.includes(flag);
    if (previous) {
      const keys = Object.keys(cell) as (keyof FrameCell)[];
      properties["Changed fields (previous frame)"] = keys.filter((key) => json(cell[key]) !== json(previous[key])).join(", ") || "none";
    }
    pairs("cell-properties", properties, { Foreground: cell.resolved_fg, Background: cell.resolved_bg });
    text("cell-json", json({ screen_sequence: frame.sequence, column: x, row: y, ...cell, codepoints, lead_column: leadColumn ?? null }));
    for (const mismatch of mismatches) {
      get("cell-mismatches").hidden = false;
      get("cell-mismatches").append(element("strong", `${mismatch.property} / stage ${mismatch.stage}`),
        element("p", `Expected: ${mismatch.operator} ${mismatch.expected}`),
        element("p", `Observed: ${mismatch.actual}${mismatch.resolved ? ` (${mismatch.resolved})` : ""}`));
    }
  }
}
