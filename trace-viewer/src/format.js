/** @import { FailureLocatorQuery, FailureMatchOccurrence, FailureTextAnchor, FailureTextStyle, OperationExpectation } from "./report.js" */
/** @import { Operation } from "./types.js" */
/** @type {readonly (keyof FailureTextStyle)[]} */
const styleKeys = ["foreground", "background", "bold", "dim", "italic", "underline_style", "underline_color", "inverse", "hidden", "strikethrough", "blink"];
const ansiNames = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "bright black", "bright red", "bright green", "bright yellow", "bright blue", "bright magenta", "bright cyan", "bright white"];
/** @param {FailureTextStyle} style */
function styleDescription(style) {
  return styleKeys.flatMap((key) => {
    const value = style[key];
    if (value == null) return [];
    if (typeof value === "string" && ["foreground", "background", "underline_color"].includes(key) && /^\d+$/.test(value)) {
      const index = Number(value);
      if (index <= 255) return [`${key.replaceAll("_", " ")}=ANSI ${index}${ansiNames[index] ? ` (${ansiNames[index]} slot)` : ""}`];
    }
    return [`${key.replaceAll("_", " ")}=${JSON.stringify(value)}`];
  }).join(", ");
}
/** @param {FailureMatchOccurrence} value */
function occurrence(value) {
  return typeof value === "object" ? `nth(${value.nth})` : value;
}
/** @param {FailureTextAnchor} value */
function anchor(value) {
  return `${value.regex ? "regex " : ""}${JSON.stringify(value.text)} (${occurrence(value.occurrence)})`;
}
/** @param {FailureLocatorQuery} query @returns {string} */
export function describeLocator(query) {
  const { selector } = query;
  let description;
  if (selector.kind === "text") {
    const text = selector.selector;
    description = `${text.regex ? "regex" : "text"} ${JSON.stringify(text.text)}`;
    if (text.whitespace === "normalize") description += " (normalized whitespace)";
    if (text.scope.after) description += ` after ${anchor(text.scope.after)}`;
    if (text.scope.before) description += ` before ${anchor(text.scope.before)}`;
  } else if (selector.kind === "style") {
    description = `cells matching ${styleDescription(selector.selector.style) || "any style"}`;
  } else if (selector.kind === "link") {
    description = selector.selector.uri === "" ? "cells with no hyperlink" : `cells linking exactly to ${JSON.stringify(selector.selector.uri)}`;
  } else if (selector.kind === "and" || selector.kind === "or") {
    const operator = selector.kind === "and" ? "cell intersection" : "cell union";
    description = `${operator} [${describeLocator(selector.selector.left)}] [${describeLocator(selector.selector.right)}]`;
  } else if (selector.kind === "filter") {
    description = `${describeLocator(selector.selector.input)} filtered by`;
    if (selector.selector.has) description += ` has [${describeLocator(selector.selector.has)}]`;
    if (selector.selector.has_not) description += ` has not [${describeLocator(selector.selector.has_not)}]`;
  } else {
    throw new Error("Unknown locator expression");
  }
  if ("full" in selector.selector && selector.selector.full) description += " in full scrollback";
  const style = styleDescription(query.style);
  if (style) description += ` with ${style}`;
  if (query.occurrence !== "any") description += ` (${occurrence(query.occurrence)})`;
  if (!query.within) return description;
  if (query.direction === "within" && (selector.kind === "style" || selector.kind === "link")) {
    return `[${describeLocator(query.within)}] whose whole match satisfies ${description}`;
  }
  return `${description} ${query.direction} [${describeLocator(query.within)}]`;
}
/** @param {OperationExpectation | undefined} value */
export function describeExpectation(value) {
  if (!value) return undefined;
  if (value.kind === "unavailable") return `Expectation not retained: ${value.reason}`;
  if (value.kind === "value") return `${value.subject}: ${value.expected}`;
  if (value.outcome === "matches") return `Resolve ${describeLocator(value.query)} (zero or more matches)`;
  const outcome = { visible: "to be visible", hidden: "to be absent", unique: "to resolve to one unambiguous match", actionable: "to be actionable" }[value.outcome];
  return `Expect ${describeLocator(value.query)} ${outcome}`;
}

/** @param {Record<string, unknown>} values */
function objectCode(values) {
  return `{ ${Object.entries(values).filter(([, value]) => value != null)
    .map(([key, value]) => `${key.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase())}: ${JSON.stringify(value)}`).join(", ")} }`;
}

/** A readable query description, not captured user source code.
 * @param {FailureLocatorQuery} query @returns {string}
 */
export function locatorCode(query) {
  const { selector } = query;
  let code;
  /** @type {Record<string, unknown>} */
  const options = {};
  if (query.within && query.direction !== "within") options.direction = query.direction;
  if ("full" in selector.selector && selector.selector.full) options.full = true;
  /** @param {string} method @param {string} argument */
  const call = (method, argument) =>
    `${method}(${argument}${Object.keys(options).length ? `, ${objectCode(options)}` : ""})`;
  switch (selector.kind) {
    case "text":
      if (selector.selector.scope.after || selector.selector.scope.before) return describeLocator(query);
      if (selector.selector.regex) options.regex = true;
      if (selector.selector.whitespace === "normalize") options.whitespace = "normalize";
      code = call("getByText", JSON.stringify(selector.selector.text));
      break;
    case "style":
      code = call("getByStyle", objectCode({ ...selector.selector.style }));
      break;
    case "link":
      code = call("getByLink", JSON.stringify(selector.selector.uri));
      break;
    case "and":
    case "or":
      code = `${locatorCode(selector.selector.left)}.${selector.kind}(${locatorCode(selector.selector.right)})`;
      break;
    case "filter": {
      const clauses = [];
      if (selector.selector.has) clauses.push(`has: ${locatorCode(selector.selector.has)}`);
      if (selector.selector.has_not) clauses.push(`hasNot: ${locatorCode(selector.selector.has_not)}`);
      code = `${locatorCode(selector.selector.input)}.filter({ ${clauses.join(", ")} })`;
      break;
    }
  }
  if (query.within) code = `${locatorCode(query.within)}.${code}`;
  if (styleKeys.some((key) => query.style[key] != null)) code += `.getByStyle(${objectCode({ ...query.style })})`;
  if (typeof query.occurrence === "object") code += `.nth(${query.occurrence.nth})`;
  else if (query.occurrence !== "any") code += `.${query.occurrence}()`;
  return code;
}

/** Shared action labels and Call fields; never infer inputs from screen text.
 * @param {Operation} operation
 */
export function describeOperation(operation) {
  const expectation = operation.expectation;
  const locator = expectation?.kind === "locator" ? locatorCode(expectation.query) : undefined;
  let name = operation.name;
  let label = locator || describeExpectation(expectation) || operation.safe_summary;
  /** @type {Record<string, string | number>} */
  const properties = {};
  const input = operation.input;
  if (!input) return { name, label, locator, properties };

  const args = input.arguments;
  switch (args.kind) {
    case "write":
    case "submit":
      label = JSON.stringify(args.data);
      properties.Text = label;
      break;
    case "key":
      name = `keyboard.${args.action}`;
      label = args.keys.map((key) => JSON.stringify(key)).join(", ");
      properties.Keys = label;
      properties.Event = args.action;
      break;
    case "mouse_click":
      name = args.target.kind === "locator" ? "locator.click" : "mouse.click";
      properties.Clicks = Math.max(1, args.clicks);
      if (args.target.kind === "text") {
        label = `onText: ${JSON.stringify(args.target.text)}`;
        properties.Target = `First text match: ${JSON.stringify(args.target.text)}`;
      } else if (args.target.kind === "position") {
        label = `(${args.target.x}, ${args.target.y})`;
        properties.Target = `Cell ${label}`;
      } else {
        properties.Target = "Resolved locator (middle cell)";
      }
      break;
    case "mouse_move":
    case "mouse_down":
    case "mouse_up":
      name = args.kind.replace("_", ".");
      label = `(${args.x}, ${args.y})`;
      properties.Target = `Cell ${label}`;
      break;
    case "mouse_drag":
      name = "mouse.drag";
      label = `(${args.x1}, ${args.y1}) to (${args.x2}, ${args.y2})`;
      properties.Path = label;
      break;
    case "mouse_scroll":
      name = "mouse.scroll";
      label = `${args.direction}, ${Math.max(1, args.amount)} steps`;
      properties.Scroll = label;
      break;
    case "unavailable":
      properties.Input = args.reason;
      return { name, label, locator, properties };
  }
  if ("options" in args) {
    properties.Button = args.options.button;
    /** @type {readonly ("ctrl" | "alt" | "shift")[]} */
    const modifiers = ["ctrl", "alt", "shift"];
    properties.Modifiers = modifiers
      .filter((modifier) => args.options[modifier]).join(" + ") || "None";
  }
  if (input.mouse_position) {
    properties["Mouse cell (column, row)"] = `${input.mouse_position.column}, ${input.mouse_position.row}`;
  }
  if (input.sent_bytes !== undefined) {
    properties["Bytes sent"] = input.sent_bytes.length;
    properties["Bytes (hex)"] = input.sent_bytes.map((byte) => byte.toString(16).padStart(2, "0")).join(" ") || "(none)";
    if (args.kind === "key" && !input.sent_bytes.length) {
      properties.Effect = "The terminal's keyboard protocol did not emit this event.";
    }
  } else {
    properties["Bytes sent"] = "Not recorded (write incomplete or evidence not retained)";
  }
  return { name, label, locator, properties };
}

/** @param {unknown} value */
export const json = (value) => JSON.stringify(value, null, 2);
/** @param {number | undefined} ms */
export const time = (ms) => ms === undefined ? "Not captured" : `${ms} ms`;
