import koffi from "koffi";
import { layouts, types } from "./abi.js";
import { enums, methods, models } from "./models.js";

const PREFIX = "__tui_test_native_error__:";
const aliases = { TextStyle: "LocatorStyle", LocatorQuery: "LocatorExpression", LocatorDirection: "LocatorStageDirection" };
const snake = (name) => name.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
const optional = (type) => type.startsWith("Option<") ? type.slice(7, -1) : undefined;
const absent = (value) => value === undefined || value === null;
const error = (kind, message, extra = {}) => new Error(PREFIX + JSON.stringify({ kind, message, ...extra }));

function integer(value, label, max = Number.MAX_SAFE_INTEGER, min = 0) {
  if (!Number.isSafeInteger(value) || value < min || value > max) {
    throw error("usage", `${label} must be an integer between ${min} and ${max}`);
  }
  return value;
}

function validate(type, value, label) {
  const inner = optional(type);
  if (inner) return absent(value) ? undefined : validate(inner, value, label);
  if (type.startsWith("Vec<")) {
    if (!Array.isArray(value)) throw new TypeError(`${label} must be an array`);
    return value.map((item, i) => validate(type.slice(4, -1), item, `${label}[${i}]`));
  }
  if (type === "(String,String)") {
    if (!Array.isArray(value) || value.length !== 2) throw new TypeError(`${label} must be a string pair`);
    return value.map((item, i) => validate("String", item, `${label}[${i}]`));
  }
  if (/^(u8|u16|u32|u64|usize|i32)$/.test(type)) {
    const max = { u8: 255, u16: 65535, u32: 4294967295, i32: 2147483647 }[type];
    return integer(value, label, max, type === "i32" ? -2147483648 : 0);
  }
  if (type === "MouseOptions") {
    const code = integer(value, "button", 255);
    if ((code & ~31) !== 0 || (code & 3) === 3) throw error("usage", `invalid mouse button code ${code}`);
    return { button: code & 3, shift: !!(code & 4), alt: !!(code & 8), ctrl: !!(code & 16) };
  }
  const primitive = { String: "string", PathBuf: "string", WhitespaceMode: "string", TraceMode: "string", FailureArtifactMode: "string", bool: "boolean", f64: "number" }[type];
  if (primitive) {
    if (typeof value !== primitive) throw new TypeError(`${label} must be a ${primitive}`);
    return value;
  }
  type = aliases[type] ?? type;
  if (enums[type]) {
    if (!Object.values(enums[type]).includes(value)) throw new TypeError(`invalid ${label}: ${String(value)}`);
    return value;
  }
  const fields = models[type];
  if (!fields) throw error("internal", `unknown native model ${type}`);
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new TypeError(`${label} must be an object`);
  if (type.startsWith("Locator")) {
    for (const key of Object.keys(value)) {
      if (!Object.hasOwn(fields, key)) throw new TypeError(`unknown locator field ${JSON.stringify(key)}`);
    }
  }
  return Object.fromEntries(Object.entries(fields).map(([key, [field, fieldType]]) => [
    field, validate(fieldType, value[key], label ? `${label}.${key}` : key),
  ]));
}

// Encoding stores raw pointers in native structs. Keep every backing allocation
// reachable until the start call returns and Rust finishes its input copy.
class Inputs {
  buffers = [];
  release() {
    this.buffers.length = 0;
  }
  string(value) {
    if (absent(value)) return { data: null, len: 0 };
    const bytes = Buffer.from(value, "utf8");
    const data = bytes.length ? bytes : Buffer.alloc(1);
    this.buffers.push(data);
    return { data, len: bytes.length };
  }
  array(type, values = []) {
    if (!values.length) return null;
    const buffer = Buffer.alloc(koffi.sizeof(types[type]) * values.length);
    this.buffers.push(buffer);
    koffi.encode(buffer, types[type], values.map((v) => this.struct(type, v)), values.length);
    return buffer;
  }
  struct(type, value) {
    if (type === "TuiString") return this.string(value);
    if (type.startsWith("TuiOptional")) {
      return { present: !absent(value), value: value ?? (type === "TuiOptionalBool" ? false : 0) };
    }
    const result = {};
    value ??= {};
    for (const [field, fieldType] of Object.entries(layouts[type])) {
      if (field.endsWith("_len")) {
        result[field] = value[field.slice(0, -4)]?.length ?? 0;
      } else if (fieldType.startsWith("*")) {
        const items = value[field] ?? [];
        result[field] = this.array(fieldType.slice(1), items);
      } else if (fieldType.startsWith("Tui")) {
        result[field] = this.struct(fieldType, value[field]);
      } else {
        result[field] = value[field] ?? (fieldType === "bool" ? false : 0);
      }
    }
    return result;
  }
}

function bytes(value) {
  if (value.data === null) return null;
  return Buffer.from(koffi.decode(value.data, "uint8_t", Number(value.len)));
}
function string(value) { return bytes(value)?.toString("utf8") ?? null; }
function array(type, pointer, length) {
  return length ? koffi.decode(pointer, types[type], Number(length)).map((value) => decode(type, value)) : [];
}
function decode(type, value) {
  if (type === "TuiString") return string(value);
  if (type.startsWith("TuiOptional")) return value.present ? Number(value.value) : null;
  if (type === "TuiColor") {
    if (value.kind === 0) return "default";
    if (value.kind === 1) return value.index;
    if (value.kind === 2) return "#" + [value.red, value.green, value.blue].map((v) => v.toString(16).padStart(2, "0")).join("");
    throw error("internal", `unknown native color kind ${value.kind}`);
  }
  const result = {};
  for (const [field, fieldType] of Object.entries(layouts[type])) {
    if (field.endsWith("_len")) continue;
    let decoded;
    if (fieldType.startsWith("*")) {
      decoded = array(fieldType.slice(1), value[field], value[`${field}_len`]);
      if (field === "modes") decoded = Object.fromEntries(decoded.map((m) => [m.name, m.enabled]));
      if (field === "palette") decoded = Object.fromEntries(decoded.map((p) => [p.key, p.value]));
    } else if (fieldType.startsWith("Tui")) decoded = decode(fieldType, value[field]);
    else decoded = typeof value[field] === "bigint" ? Number(value[field]) : value[field];
    result[field === "character" ? "char" : field] = decoded;
  }
  return result;
}

function resultValue(result, selector) {
  switch (selector) {
    case "Unit": return undefined;
    case "Open": return decode("TuiOpenResult", result.open);
    case "State": return decode("TuiState", result.state);
    case "Cursor": return decode("TuiCursor", result.cursor);
    case "Size": return decode("TuiSize", result.size);
    case "Cells": return array("TuiCell", result.cells, result.cells_len);
    case "Matches": return array("TuiMatch", result.matches, result.matches_len);
    case "BellEvents": return array("TuiBellEvent", result.bells, result.bells_len);
    case "Sessions": return array("TuiString", result.strings, result.strings_len);
    case "BellCount": return Number(result.number);
    case "ExitCode": return decode("TuiOptionalI32", result.exit_code);
    case "Snapshot": {
      const value = Object.values(enums.SnapshotResult)[result.snapshot];
      if (value === undefined) throw error("internal", `unknown snapshot result ${result.snapshot}`);
      return value;
    }
    case "PackedScreen": return { ...decode("TuiSize", result.size), utf8: bytes(result.text) };
    case "Text": case "Command": case "Output": case "Cwd": case "Title":
    case "Clipboard": case "Recording": case "Screenshot": return string(result.text);
    default: throw error("internal", `unknown native result ${selector}`);
  }
}

function locator(expression) {
  for (const [index, node] of expression.nodes.entries()) {
    for (const key of ["within", "left", "right", "input", "has", "has_not"]) {
      if (!absent(node[key])) integer(node[key], "locator reference", 255);
    }
    if (!absent(node.nth)) {
      if (!absent(node.occurrence) && node.occurrence !== "nth") {
        throw error("usage", `nodes[${index}].nth cannot be used with occurrence '${node.occurrence}'`);
      }
      node.occurrence = { nth: node.nth };
    } else {
      node.occurrence ??= "any";
      if (node.occurrence === "nth") throw error("usage", `nodes[${index}].nth requires an nth index`);
      if (!["any", "unique", "first", "last"].includes(node.occurrence)) throw error("usage", `invalid occurrence '${node.occurrence}'`);
    }
    delete node.nth;
  }
  integer(expression.root, "locator root", 255);
  return { json: JSON.stringify(expression) };
}

const mouseDefault = () => ({ button: 0, shift: false, alt: false, ctrl: false });
const pairs = (values) => values?.map(([key, value]) => ({ key, value }));

export function createAdapter(functions) {
  const reads = [];
  let activeReads = 0;

  function read(name, args, selector, input) {
    return new Promise((resolve, reject) => {
      const run = () => {
        activeReads++;
        const finish = (failure, pointer) => {
          try {
            if (failure) reject(failure);
            else resolve(consume(pointer, selector));
          } catch (failure) {
            reject(failure);
          } finally {
            input?.release();
            activeReads--;
            reads.shift()?.();
          }
        };
        try {
          functions[name].async(...args, finish);
        } catch (failure) {
          finish(failure);
        }
      };
      // Bound submissions, not the number of callers. Koffi rejects excess
      // in-flight calls; the queue preserves promises without changing its limits.
      if (activeReads < 4) run();
      else reads.push(run);
    });
  }

  function consume(pointer, selector) {
    if (pointer === null) throw error("internal", "native call returned a null result");
    try {
      const result = koffi.decode(pointer, types.TuiResult);
      if (result.error_kind) {
        const kinds = { 1: "assertion", 2: "usage", 3: "no_session", 5: "internal" };
        const kind = kinds[result.error_kind];
        if (!kind) throw error("internal", `unknown native error kind ${result.error_kind}`);
        const details = string(result.error_details);
        const artifact = string(result.error_artifact);
        throw error(kind, string(result.error_message), {
          ...(details === null ? {} : { details: JSON.parse(details) }),
          ...(artifact === null ? {} : { artifact: JSON.parse(artifact) }),
        });
      }
      return resultValue(result, selector);
    } finally {
      functions.tui_result_free(pointer);
    }
  }

  function call(name, args, selector, input, short = false) {
    if (short) return read(name, args, selector, input);
    let task;
    try {
      task = functions[`${name}_start`](...args);
    } finally {
      // This post-call access retains nested buffers through native marshaling.
      // Rust owns its input copy when the synchronous start function returns.
      input?.release();
    }
    if (task === null) return Promise.reject(error("internal", "native call returned a null task"));
    return new Promise((resolve, reject) => {
      const poll = () => {
        let pointer;
        try {
          pointer = functions.tui_task_poll(task);
          if (pointer === null) { setTimeout(poll, 1); return; }
          resolve(consume(pointer, selector));
        } catch (failure) { reject(failure); }
        finally {
          if (pointer !== null) functions.tui_task_free(task);
        }
      };
      poll();
    });
  }

  class NativeSession {
    #name;
    #context;
    #directory;
    constructor(name, recording, artifacts, trace) {
      this.#name = validate("String", name, "name");
      recording = validate("Option<AutomaticRecordingOptions>", recording, "");
      artifacts = validate("Option<FailureArtifactOptions>", artifacts, "");
      trace = validate("Option<TraceOptions>", trace, "");
      this.#directory = recording?.directory;
      this.#context = { name };
      if (artifacts) Object.assign(this.#context, {
        artifact_directory: artifacts.directory,
        artifact_mode: artifacts.mode ?? "all",
        artifact_include_recording: artifacts.include_recording ?? false,
      });
      if (trace) Object.assign(this.#context, { trace_directory: trace.directory, trace_mode: trace.mode ?? "off" });
    }
    name() { return this.#name; }
    async open(options) { return this.#open("open", validate("OpenOptions", options ?? {}, "")); }
    async run(options) { return this.#open("run", validate("RunOptions", options, "")); }
    #open(method, options) {
      const input = new Inputs();
      const context = input.string(JSON.stringify({ ...this.#context, screen_history_limit: options.screen_history_limit }));
      const native = input.struct("TuiOpenOptions", {
        ...options, env: pairs(options.env), colors: pairs(options.profile_colors),
        scrollback: options.profile_scrollback, recording_directory: this.#directory,
      });
      const args = [context, native];
      if (method === "run") args.push(input.string(options.program), input.array("TuiString", options.args), options.args?.length ?? 0);
      return call(`tui_${method}`, args, "Open", input);
    }
    async close(failed) {
      failed = validate("Option<bool>", failed, "failed");
      const input = new Inputs();
      return call("tui_close", [input.string(JSON.stringify(this.#context)), input.struct("TuiOptionalBool", failed)], "Unit", input);
    }
    async panicProbe() { return call("tui_panic_probe", [], "Unit"); }
    async #invoke(method, args) {
      const spec = methods[method];
      const values = Object.fromEntries(spec.args.map(([name, type], i) => [name, validate(type, args[i], name === "options" ? "" : name)]));
      const input = new Inputs();
      const s = (v) => input.string(v);
      const u = (v) => input.struct("TuiOptionalU64", v);
      const wait = (v) => input.struct("TuiWaitOptions", { ...v, not_: v.not });
      const options = values.options ?? {};
      const v = values;
      let name = `tui_${snake(method)}`;
      let native;
      if (["press", "keyDown", "repeat", "keyUp"].includes(method)) {
        name = "tui_key";
        native = [input.array("TuiString", v.keys), v.keys.length, ["press", "keyDown", "repeat", "keyUp"].indexOf(method)];
      } else switch (method) {
        case "text": case "packedScreen": native = [v.full ?? false]; break;
        case "cells": native = [v.x, v.y, v.w ?? 1, v.h ?? 1]; break;
        case "write": native = [s(v.data)]; break;
        case "type": native = [s(v.text)]; break;
        case "submit": native = [s(v.data)]; break;
        case "signal": native = [s(v.name)]; break;
        case "mouseClick": native = [u(options.x), u(options.y), s(options.on_text), options.button ?? mouseDefault(), options.clicks ?? 1]; break;
        case "mouseDown": case "mouseUp": native = [v.x, v.y, v.button ?? mouseDefault()]; break;
        case "mouseDrag": native = [v.x1, v.y1, v.x2, v.y2, v.button ?? mouseDefault()]; break;
        case "mouseScroll": native = [s(v.direction), v.amount ?? 3]; break;
        case "waitTitle": case "expectTitle": case "waitClipboard": native = [s(v.text), wait(options)]; break;
        case "waitIdle": case "waitCommand": case "waitExit": case "waitReady": case "waitBell": native = [u(v.timeoutMs)]; break;
        case "findLocator": native = [input.struct("TuiQuery", locator(v.expression)), v.requireOne ?? false]; break;
        case "waitLocator": case "expectLocator": native = [input.struct("TuiQuery", locator(v.expression)), wait({ not: v.not, timeout_ms: v.timeoutMs })]; break;
        case "clickLocator": native = [input.struct("TuiQuery", locator(v.expression)), v.button ?? mouseDefault(), v.clicks ?? 1, u(v.timeoutMs)]; break;
        case "highlightLocator": native = [input.struct("TuiQuery", locator(v.expression)), u(v.timeoutMs)]; break;
        case "expectExitCode": native = [v.code, u(v.timeoutMs)]; break;
        case "expectBellCount": native = [v.count, u(v.timeoutMs)]; break;
        case "expectOutput": native = [s(v.text), v.regex ?? false]; break;
        case "snapshot": native = [s(v.name), options.update ?? false, options.include_style ?? false, options.include_title ?? false, s(options.cwd)]; break;
        case "screenshot": native = [options.full ?? false, s(options.path), input.struct("TuiOptionalF64", options.zoom), s(options.background), options.transparent ?? false]; break;
        case "startRecording": native = [input.struct("TuiRecordingOptions", options)]; break;
        default: native = Object.values(v);
      }
      return call(name, [s(JSON.stringify(this.#context)), ...native], spec.result, input, spec.short);
    }
    static {
      for (const method of Object.keys(methods)) {
        Object.defineProperty(this.prototype, method, {
          configurable: true, writable: true,
          value: function (...args) { return this.#invoke(method, args); },
        });
      }
    }
  }
  return {
    NativeSession,
    sessions: async () => call("tui_sessions", [], "Sessions"),
    recording: async (name) => {
      const input = new Inputs();
      return call("tui_recording", [input.string(validate("String", name, "name"))], "Recording", input);
    },
    closeAll: async () => call("tui_close_all", [], "Unit"),
    closeAllSync: () => consume(functions.tui_close_all(), "Unit"),
  };
}
