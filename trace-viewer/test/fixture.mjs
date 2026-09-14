import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

export const hostile = '</script><script>globalThis.reportPwned=true</script>\n<img src=x onerror="globalThis.reportPwned=true"> & $& /* REPORT_JS */ null /* REPORT_DATA */';
export const geometry = { grid_x: 7, grid_y: 11, cell_width: 10, cell_height: 20 };
export const query = (text, style = {}) => ({
  selector: { kind: "text", selector: { text, regex: false, full: false, whitespace: "exact", scope: {} } },
  occurrence: "any", direction: "within", style,
});
const blank = {
  char: " ", width: 1, fg: "default", bg: "default", underline_color: "default", underline_style: "none",
  resolved_fg: "#ffffff", resolved_bg: "#000000", resolved_underline_color: "#ffffff", flags: [],
};

function frame(sequence, cols, rows, text) {
  return {
    sequence, first_seen_ms: sequence * 10, last_seen_ms: sequence * 10 + 9, repeat_count: 1,
    size: { cols, rows }, cursor: { column: 3, row: 0, shape: "block", visible: true }, changes: ["text", "style"], text,
    cells: [
      blank, { ...blank, char: "A", fg: 1, resolved_fg: "#112233" }, { ...blank, char: "\u4f60", width: 2 },
      { ...blank, char: "", width: 0 }, { ...blank, char: "e\u0301", flags: ["bold"], link: "javascript:globalThis.reportPwned=true", link_id: "test" },
    ],
    grid: Array.from({ length: rows }, (_, row) => Array.from({ length: cols }, (_, col) => row === 0 && col < 4 ? col + 1 : 0)),
    svg: `<svg xmlns="http://www.w3.org/2000/svg" width="${cols * 10 + 14}" height="${rows * 20 + 22}" viewBox="0 0 ${cols * 10 + 14} ${rows * 20 + 22}"><rect width="100%" height="100%" fill="#000"/><text x="7" y="26" fill="#112233" font-family="monospace">A\u4f60e\u0301</text></svg>`,
  };
}

function operation(sequence, name, screen, expectation, result = "ok", input) {
  return {
    sequence, name, screen_before: Math.max(0, screen - 1), screen_at_return: screen,
    started_ms: sequence * 5, ended_ms: sequence * 5 + 4, result,
    safe_summary: name, is_assertion: !!expectation, expectation, input,
  };
}

export function attachment(name, content) {
  const bytes = Buffer.from(content);
  return { name, bytes: bytes.length, data: bytes.toString("base64") };
}

export function fixture() {
  const frames = [frame(1, 20, 4, "READY"), frame(2, 20, 4, "WORKING"), frame(3, 24, 5, "A\u4f60e\u0301")];
  const ready = { kind: "locator", query: query("READY"), outcome: "visible" };
  const mismatch = {
    location: { column: 0, row: 7 }, grapheme: "A", property: "foreground", operator: "equals",
    expected: "2", actual: "1", resolved: "#112233", reason: "color mismatch",
  };
  const stage = {
    stage_index: 1, expression_path: "root", evaluations: 1, mode: "parent_style_filter", direction: "within",
    requested_occurrence: "any", effective_occurrence: "any", occurrence_source: "explicit",
    input_candidate_count: 1, raw_candidate_count: 1, style_candidate_count: 0, selected_count: 0,
    candidates: [], candidates_truncated: false, mismatches_truncated: false,
  };
  return structuredClone({
    details: {
      schema_version: 1, signature: "sha256:test", reason: "locator_no_match", summary: hostile, outcome: "failed", truncated: false,
      operation: { name: "locator.expect", elapsed_ms: 20, timeout_ms: 20, started_screen_sequence: 2, failed_screen_sequence: 3 },
      terminal: {
        size: frames[2].size, cursor: frames[2].cursor, title: hostile, last_visual_change_ms: 30, unchanged_for_ms: 9,
        screen_history: { limit: 10, dropped_screen_count: 1, dropped_checkpoint_count: 2, dropped_row_count: 0, screens: frames },
      },
      runtime: {
        session_name: "deployment-wizard", shell: "pwsh", backend: "alacritty", target_os: "test", target_arch: "test",
        tui_test_version: "fixture", terminal_profile_fingerprint: "sha256:test",
        timeouts: { text: 5000, idle: 5000, command: 30000, exit: 30000, ready: 30000 },
      },
      process: { pid: 123, state: "running", cancelled: false, ready: true, command_running: false },
      recent_operations: [
        operation(1, "evicted check", 999, { kind: "unavailable", reason: "evidence limit" }),
        operation(2, "ready", 1, ready),
        operation(3, "ready again", 1, ready),
        operation(4, "submit", 2, undefined, "ok", { arguments: { kind: "submit", data: "DEPLOY staging" }, sent_bytes: [65, 13] }),
        operation(5, "passing on failure screen", 3, { kind: "locator", query: query("A"), outcome: "visible" }),
        operation(6, "locator.expect", 3, { kind: "locator", query: query("A", { foreground: "2" }), outcome: "visible" }, "assertion"),
      ],
      locator: {
        search_scope: "full", viewport_origin_y: 7, final_candidate_count: 0, failure_stage: 1, selected: [],
        stages: [
          { ...stage, stage_index: 0, mismatches: [{ ...mismatch, location: { column: 3, row: 7 } }] },
          { ...stage, mismatches: [mismatch, { ...mismatch, property: "underline_color" }] },
        ],
      },
      hints: [{ code: "inspect_style_mismatch", message: "The selector found candidate text, but its requested style did not match." }],
      context: { test: hostile },
    },
    timeline: { geometry, frames, failure_screen_sequence: 3 },
    explanation: {
      title: "Foreground mismatch", expected: '"style": foreground equals ANSI 2 (green slot)',
      actual: "Candidate matched, but foreground was ANSI 1 (red slot); rendered #112233.",
      note: "Select a highlighted cell for its exact comparison.",
    },
    files: [{ path: "session.cast", status: "omitted", reason: "inclusion is opt-in" }], errors: [],
    attachments: [
      attachment("current.svg", frames[2].svg), attachment("current.txt", frames[2].text),
      attachment("failure.md", hostile), attachment("timeline.json", JSON.stringify({ frames })),
      attachment("failure.json", JSON.stringify({ summary: hostile })),
    ],
  });
}

export async function renderReport(data) {
  const base = new URL("../../crates/tui-test/assets/trace-viewer/", import.meta.url);
  const [template, css, js] = await Promise.all(["report.html", "report.min.css", "report.min.js"].map((name) =>
    readFile(new URL(name, base), "utf8")));
  const json = JSON.stringify(data).replace(/[<>&\u2028\u2029]/g,
    (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`);
  return template.replace("/* REPORT_CSS */", () => css).replace("/* REPORT_JS */", () => js)
    .replace("null /* REPORT_DATA */", () => json);
}

export async function writeReport(filename, data = fixture()) {
  await mkdir(path.dirname(filename), { recursive: true });
  await writeFile(filename, await renderReport(data));
  return filename;
}
