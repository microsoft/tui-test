/** @typedef {(value: unknown, path: string) => void} Validator */

/** @param {unknown} value @param {string} path @returns {asserts value is Record<string, unknown>} */
function object(value, path) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) invalid(path, "an object");
}
/** @param {string} path @param {string} expected @returns {never} */
function invalid(path, expected) {
  throw new Error(`${path}: expected ${expected}`);
}
/** @param {(value: unknown) => boolean} predicate @param {string} expected @returns {Validator} */
const scalar = (predicate, expected) => (value, path) => {
  if (!predicate(value)) invalid(path, expected);
};
const text = scalar((value) => typeof value === "string", "a string");
const bool = scalar((value) => typeof value === "boolean", "a boolean");
const number = scalar((value) => typeof value === "number" && Number.isFinite(value) && value >= 0, "a non-negative finite number");
const integer = scalar((value) => Number.isSafeInteger(value) && Number(value) >= 0, "a non-negative safe integer");
const positive = scalar((value) => typeof value === "number" && Number.isFinite(value) && value > 0, "a positive finite number");
const dimension = scalar((value) => Number.isSafeInteger(value) && Number(value) > 0, "a positive safe integer");
/** @param {Validator} validate @returns {Validator} */
const optional = (validate) => (value, path) => { if (value !== undefined) validate(value, path); };
/** @param {Validator} validate @returns {Validator} */
const nullable = (validate) => optional((value, path) => { if (value !== null) validate(value, path); });
/** @param {readonly unknown[]} values @returns {Validator} */
const choice = (values) => scalar((value) => values.includes(value), values.map(String).join(" or "));
/** @param {Validator} validate @returns {Validator} */
const list = (validate) => (value, path) => {
  if (!Array.isArray(value)) invalid(path, "an array");
  value.forEach((item, index) => validate(item, `${path}[${index}]`));
};
/** @param {Record<string, Validator>} fields @returns {Validator} */
const shape = (fields) => (value, path) => {
  object(value, path);
  for (const [key, validate] of Object.entries(fields)) validate(value[key], `${path}.${key}`);
};
/** @param {Record<string, Validator>} variants @returns {Validator} */
const tagged = (variants) => (value, path) => {
  object(value, path);
  const kind = value.kind;
  if (typeof kind !== "string" || !Object.hasOwn(variants, kind)) invalid(`${path}.kind`, Object.keys(variants).join(" or "));
  variants[kind](value, path);
};
const position = shape({ column: integer, row: integer });
const size = shape({ cols: dimension, rows: dimension });
const cursor = shape({ column: integer, row: integer, shape: text, visible: bool });
/** @type {Validator} */
const occurrence = (value, path) => typeof value === "string"
  ? choice(["any", "unique", "first", "last"])(value, path) : shape({ nth: integer })(value, path);
const style = shape({
  foreground: nullable(text), background: nullable(text), underline_color: nullable(text), underline_style: nullable(text),
  bold: nullable(bool), dim: nullable(bool), italic: nullable(bool), inverse: nullable(bool), hidden: nullable(bool),
  strikethrough: nullable(bool), blink: nullable(bool),
});
const anchor = shape({ text, regex: bool, occurrence });
const direction = choice(["within", "before", "after"]);
/** @type {Validator} */
function locator(value, path) {
  if (path.split(".").length > 128) invalid(path, "a locator nested at most 64 levels deep");
  shape({ selector, occurrence, direction, style, within: nullable(locator) })(value, path);
}
const selector = tagged({
  text: shape({ selector: shape({ text, regex: bool, full: bool, whitespace: choice(["exact", "normalize"]),
    scope: shape({ after: nullable(anchor), before: nullable(anchor) }) }) }),
  style: shape({ selector: shape({ style, full: bool }) }),
  link: shape({ selector: shape({ uri: text, full: bool }) }),
  and: shape({ selector: shape({ left: locator, right: locator }) }),
  or: shape({ selector: shape({ left: locator, right: locator }) }),
  filter: shape({ selector: shape({ input: locator, has: nullable(locator), has_not: nullable(locator) }) }),
});
const expectation = tagged({
  locator: shape({ query: locator, outcome: choice(["matches", "visible", "hidden", "unique", "actionable"]) }),
  value: shape({ subject: text, expected: text }),
  unavailable: shape({ reason: text }),
});
const mouseOptions = shape({ button: choice(["left", "middle", "right"]), alt: bool, ctrl: bool, shift: bool });
const mousePosition = shape({ x: integer, y: integer });
const mouseButton = shape({ x: integer, y: integer, options: mouseOptions });
const input = shape({
  arguments: tagged({
    write: shape({ data: text }), submit: shape({ data: text }),
    key: shape({ keys: list(text), action: choice(["press", "down", "repeat", "up"]) }),
    mouse_click: shape({
      target: tagged({ position: mousePosition, text: shape({ text }), locator: shape({}) }),
      options: mouseOptions, clicks: integer,
    }),
    mouse_move: mousePosition, mouse_down: mouseButton, mouse_up: mouseButton,
    mouse_drag: shape({ x1: integer, y1: integer, x2: integer, y2: integer, options: mouseOptions }),
    mouse_scroll: shape({ direction: text, amount: integer }),
    unavailable: shape({ reason: text }),
  }),
  mouse_position: optional(position),
  sent_bytes: optional(list(scalar((value) => Number.isInteger(value) && Number(value) >= 0 && Number(value) <= 255, "a byte"))),
});
const operation = shape({
  sequence: integer, name: text, started_ms: number, ended_ms: number, screen_before: integer, screen_at_return: integer,
  safe_summary: text, result: text, is_assertion: optional(bool), expectation: optional(expectation), input: optional(input),
});
const color = scalar((value) => typeof value === "string" || (Number.isInteger(value) && Number(value) >= 0 && Number(value) <= 255), "a color string or ANSI index");
const cell = shape({
  char: text, width: choice([0, 1, 2]), fg: color, bg: color, underline_color: color, underline_style: text,
  flags: list(text), resolved_fg: text, resolved_bg: text, resolved_underline_color: text, link: optional(text), link_id: optional(text),
});
const screenFields = {
  sequence: integer, first_seen_ms: number, last_seen_ms: number, repeat_count: integer,
  size, cursor, text, changes: list(text), title: optional(text),
};
const mismatch = shape({
  location: position, grapheme: text, property: text, operator: text, expected: text, actual: text, resolved: optional(text), reason: text,
});
const stage = shape({
  stage_index: integer, expression_path: text, evaluations: integer, mode: text, direction,
  requested_occurrence: occurrence, effective_occurrence: occurrence, occurrence_source: choice(["explicit", "action_default"]),
  input_candidate_count: integer, raw_candidate_count: integer, style_candidate_count: integer, selected_count: integer,
  candidates_truncated: bool, mismatches_truncated: bool, mismatches: optional(list(mismatch)), selector: optional(selector),
});
const report = shape({
  details: shape({
    schema_version: choice([1]), signature: text, reason: text, summary: text, outcome: optional(choice(["passed", "failed"])), truncated: bool,
    operation: shape({ name: text, timeout_ms: optional(number), elapsed_ms: number, started_screen_sequence: integer, failed_screen_sequence: integer }),
    recent_operations: optional(list(operation)),
    locator: optional(shape({ search_scope: text, viewport_origin_y: integer, final_candidate_count: integer, failure_stage: optional(integer), stages: list(stage) })),
    hints: optional(list(shape({ code: text, message: text }))),
    terminal: optional(shape({
      size, cursor, title: optional(text), last_visual_change_ms: number, unchanged_for_ms: number,
      screen_history: shape({
        limit: integer, dropped_screen_count: integer, dropped_row_count: integer, dropped_checkpoint_count: optional(integer),
        screens: list(shape(screenFields)), checkpoints: optional(list(shape(screenFields))),
      }),
    })),
    runtime: optional(shape({
      session_name: optional(text), shell: optional(text), backend: text, target_os: text, target_arch: text, tui_test_version: text,
      timeouts: optional(shape({ text: number, idle: number, command: number, exit: number, ready: number })),
    })),
    process: optional(shape({ pid: optional(integer), state: text, cancelled: bool, ready: bool, command_running: bool })),
    recording: optional(shape({
      mode: choice(["disabled", "on-failure", "always"]), status: text, failure_offset_ms: number, ephemeral: bool, reason: optional(text),
    })),
  }),
  timeline: shape({
    schema_version: optional(choice([1])), failure_screen_sequence: integer,
    geometry: shape({ grid_x: number, grid_y: number, cell_width: positive, cell_height: positive }),
    frames: list(shape({ ...screenFields, cells: list(cell), grid: list(list(integer)), svg: optional(text), omission: optional(text) })),
  }),
  explanation: shape({ title: text, expected: text, actual: text, note: text }),
  files: list(shape({ path: text, status: text, reason: optional(text) })),
  errors: list(text),
  attachments: list(shape({ name: text, bytes: integer, data: text })),
});

/** Validate consumed evidence at the boundary; unknown fields remain in raw evidence.
 * @param {unknown} value @returns {asserts value is import("./types.js").ReportData}
 */
export function validateReport(value) {
  report(value, "report");
}
