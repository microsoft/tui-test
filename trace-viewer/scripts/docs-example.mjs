import { mkdir, readFile, writeFile } from "node:fs/promises";
import { parseReport } from "../src/model.js";

const cols = 64;
const rows = 12;
const geometry = { grid_x: 20, grid_y: 40, cell_width: 9, cell_height: 18 };
const palette = { default: "#d8d8e8", "1": "#ff6b6b", "2": "#7cd67c", "3": "#f0ca6c" };
const escape = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

function frame(sequence, time, status, color) {
  const lines = [
    "Deployment dashboard",
    "",
    "Environment  staging",
    "Service      sample-api",
    "Version      1.4.2",
    "",
    "[ Deploy ]   [ Cancel ]",
    "",
    `Status: ${status}`,
    "",
    sequence === 3 ? "Deployment finished in 1.3s." : "Select Deploy to continue.",
  ];
  const cells = [];
  const indices = new Map();
  const grid = Array.from({ length: rows }, (_, row) =>
    Array.from({ length: cols }, (_, column) => {
      const foreground = row === 8 && column >= 8 ? color : "default";
      const cell = {
        char: lines[row]?.[column] ?? " ", width: 1,
        fg: foreground === "default" ? "default" : Number(foreground),
        bg: "default", underline_color: "default", underline_style: "none",
        resolved_fg: palette[foreground], resolved_bg: "#111214",
        resolved_underline_color: palette[foreground], flags: [],
      };
      const key = JSON.stringify(cell);
      if (!indices.has(key)) {
        indices.set(key, cells.length);
        cells.push(cell);
      }
      return indices.get(key);
    }),
  );
  const text = lines.join("\n");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="616" height="276" viewBox="0 0 616 276">
<rect width="616" height="276" rx="8" fill="#111214"/>
<text x="20" y="23" fill="#8c959f" font-family="monospace" font-size="12">sample-app</text>
${grid.map((line, row) => line.map((index, column) => {
    const cell = cells[index];
    return cell.char === " " ? "" : `<text x="${20 + column * 9}" y="${54 + row * 18}" fill="${cell.resolved_fg}" font-family="monospace" font-size="15">${escape(cell.char)}</text>`;
  }).join("")).join("\n")}
</svg>`;
  return {
    sequence, first_seen_ms: time, last_seen_ms: time + 300, repeat_count: 1,
    size: { cols, rows }, cursor: { column: 0, row: 11, shape: "block", visible: false },
    changes: ["text", "style"], text, cells, grid, svg,
  };
}

const frames = [
  frame(1, 0, "Ready", "2"),
  frame(2, 600, "Deploying", "3"),
  frame(3, 1300, "Deployed", "1"),
];
const query = (text, style = {}) => ({
  selector: { kind: "text", selector: { text, regex: false, full: false, whitespace: "exact", scope: {} } },
  occurrence: "unique", direction: "within", style,
});
const expectation = (text, style) => ({ kind: "locator", query: query(text, style), outcome: "visible" });
const operation = (sequence, name, screen, started, ended, extra = {}) => ({
  sequence, name, screen_before: Math.max(1, screen - 1), screen_at_return: screen,
  started_ms: started, ended_ms: ended, result: "ok", safe_summary: name,
  is_assertion: false, ...extra,
});
const match = {
  text: "Deployed", start: { row: 8, column: 8 }, end: { row: 8, column: 16 },
  spans: [{ row: 8, start: 8, end: 16 }],
};
const mismatches = [..."Deployed"].map((grapheme, index) => ({
  location: { row: 8, column: 8 + index }, grapheme,
  property: "foreground", operator: "equals", expected: "2", actual: "1",
  resolved: palette["1"], reason: "color mismatch",
}));
const data = {
  details: {
    schema_version: 1, signature: "documentation-example",
    reason: "locator_style_mismatch", summary: '"Deployed" appeared in red; the test expected green.',
    outcome: "failed", truncated: false,
    operation: { name: "locator.expect", elapsed_ms: 5000, timeout_ms: 5000, started_screen_sequence: 2, failed_screen_sequence: 3 },
    terminal: {
      size: { cols, rows }, cursor: frames[2].cursor, title: "Deployment dashboard",
      last_visual_change_ms: 1300, unchanged_for_ms: 4600,
      screen_history: { limit: 10, dropped_screen_count: 0, dropped_checkpoint_count: 0, dropped_row_count: 0, screens: frames },
    },
    runtime: {
      session_name: "deployment-example", shell: "sample-app", backend: "alacritty",
      target_os: "example", target_arch: "example", tui_test_version: "documentation example",
      timeouts: { text: 5000, idle: 5000, command: 30000, exit: 30000, ready: 30000 },
    },
    process: { state: "running", cancelled: false, ready: true, command_running: false },
    recent_operations: [
      operation(1, "run", 1, 0, 100),
      operation(2, "locator.expect", 1, 100, 110, { is_assertion: true, expectation: expectation("Ready") }),
      operation(3, "locator.click", 2, 600, 620, {
        expectation: { kind: "locator", query: query("Deploy"), outcome: "actionable" },
        input: {
          arguments: {
            kind: "mouse_click", target: { kind: "locator" },
            options: { button: "left", alt: false, ctrl: false, shift: false }, clicks: 1,
          },
          mouse_position: { column: 4, row: 6 },
        },
      }),
      operation(4, "locator.expect", 3, 900, 5900, { result: "assertion", is_assertion: true, expectation: expectation("Deployed", { foreground: "2" }) }),
    ],
    locator: {
      search_scope: "visible", viewport_origin_y: 0, final_candidate_count: 0,
      failure_stage: 0, selected: [],
      stages: [{
        stage_index: 0, expression_path: "root", evaluations: 100,
        mode: "text", direction: "within", requested_occurrence: "unique",
        effective_occurrence: "unique", occurrence_source: "explicit",
        input_candidate_count: 0, raw_candidate_count: 1, style_candidate_count: 0,
        selected_count: 0, candidates: [match], mismatches,
        candidates_truncated: false, mismatches_truncated: false,
      }],
    },
    hints: [{ code: "inspect_style_mismatch", message: "The text matched, but its foreground color did not." }],
    context: { test: "deployment status uses the success color" },
  },
  timeline: { geometry, frames, failure_screen_sequence: 3 },
  explanation: {
    title: "Foreground mismatch",
    expected: '"Deployed" with a green foreground',
    actual: '"Deployed" uses a red foreground (#ff6b6b).',
    note: "Select a highlighted cell to inspect the color comparison.",
  },
  files: [], errors: [], attachments: [],
};
const attach = (name, content) => {
  const bytes = Buffer.from(content);
  data.attachments.push({ name, bytes: bytes.length, data: bytes.toString("base64") });
};
attach("current.svg", frames[2].svg);
attach("current.txt", frames[2].text);
attach("trace.md", '# Example: deployment status\n\nThe "Deployed" label is red. The test expects green.\n');
attach("timeline.json", JSON.stringify(data.timeline));
attach("trace.json", JSON.stringify(data.details));
const cast = [
  JSON.stringify({ version: 2, width: cols, height: rows, title: "Deployment dashboard", env: { TERM: "xterm-256color" } }),
  ...frames.map((screen) => JSON.stringify([screen.first_seen_ms / 1000, "o", "\x1b[2J\x1b[H" +
    screen.text.replace(/^Status: (.+)$/m, (_, status) => `Status: \x1b[${screen.sequence === 3 ? 31 : screen.sequence === 2 ? 33 : 32}m${status}\x1b[0m`).replaceAll("\n", "\r\n")])),
].join("\n") + "\n";
attach("session.cast", cast);
parseReport(JSON.stringify(data));

const assets = new URL("../../crates/tui-test/assets/trace-viewer/", import.meta.url);
const [template, css, js] = await Promise.all(["report.html", "report.min.css", "report.min.js"].map((file) => readFile(new URL(file, assets), "utf8")));
const json = JSON.stringify(data).replace(/[<>&\u2028\u2029]/g, (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`);
const html = template.replace("/* REPORT_CSS */", () => css).replace("/* REPORT_JS */", () => js).replace("null /* REPORT_DATA */", () => json);
const output = new URL("../../docs/website/assets/trace-viewer/", import.meta.url);
await mkdir(output, { recursive: true });
await writeFile(new URL("index.html", output), html);
console.log("Wrote the documentation trace example with the current bundled viewer.");
