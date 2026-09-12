import type {
  FailureLocatorQuery, FailureMatchOccurrence, FailureTextAnchor, FailureTextStyle, OperationExpectation,
} from "../../../../../bindings/js/src/types.js";

const styleKeys = ["foreground", "background", "bold", "dim", "italic", "underline_style", "underline_color", "inverse", "hidden", "strikethrough", "blink"] as const;
const ansiNames = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "bright black", "bright red", "bright green", "bright yellow", "bright blue", "bright magenta", "bright cyan", "bright white"];
function styleDescription(style: FailureTextStyle): string {
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
function occurrence(value: FailureMatchOccurrence): string {
  return typeof value === "object" ? `nth(${value.nth})` : value;
}
function anchor(value: FailureTextAnchor): string {
  return `${value.regex ? "regex " : ""}${JSON.stringify(value.text)} (${occurrence(value.occurrence)})`;
}
export function describeLocator(query: FailureLocatorQuery): string {
  const { selector } = query;
  let description: string;
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
export function describeExpectation(value?: OperationExpectation): string | undefined {
  if (!value) return undefined;
  if (value.kind === "unavailable") return `Expectation not retained: ${value.reason}`;
  if (value.kind === "value") return `${value.subject}: ${value.expected}`;
  if (value.outcome === "matches") return `Resolve ${describeLocator(value.query)} (zero or more matches)`;
  const outcome = { visible: "to be visible", hidden: "to be absent", unique: "to resolve to one unambiguous match", actionable: "to be actionable" }[value.outcome];
  return `Expect ${describeLocator(value.query)} ${outcome}`;
}

function objectCode(values: Record<string, unknown>): string {
  return `{ ${Object.entries(values).filter(([, value]) => value != null)
    .map(([key, value]) => `${key.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())}: ${JSON.stringify(value)}`).join(", ")} }`;
}

/** A readable query description, not captured user source code. */
export function locatorCode(query: FailureLocatorQuery): string {
  const { selector } = query;
  let code: string;
  const options: Record<string, unknown> = {};
  if (query.within && query.direction !== "within") options.direction = query.direction;
  if ("full" in selector.selector && selector.selector.full) options.full = true;
  const call = (method: string, argument: string) =>
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
