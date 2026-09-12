import type {
  FailureLocatorQuery, FailureMatchOccurrence, FailureTextAnchor, FailureTextStyle, OperationExpectation,
} from "../../../../../bindings/js/src/types.js";

const styleKeys = ["foreground", "background", "bold", "dim", "italic", "underline_style", "underline_color", "inverse", "hidden", "strikethrough", "blink", "link"] as const;
const ansiNames = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "bright black", "bright red", "bright green", "bright yellow", "bright blue", "bright magenta", "bright cyan", "bright white"];
function styleDescription(style: FailureTextStyle): string {
  return styleKeys.flatMap((key) => {
    const value = style[key];
    if (value == null) return [];
    if (key === "link") return [value === "" ? "no hyperlink" : `link=${JSON.stringify(value)}`];
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
  } else {
    description = `cells matching ${styleDescription(selector.selector.style) || "any style"}`;
  }
  if (selector.selector.full) description += " in full scrollback";
  const style = styleDescription(query.style);
  if (style) description += ` with ${style}`;
  if (query.occurrence !== "any") description += ` (${occurrence(query.occurrence)})`;
  return query.within ? `${description} ${query.direction} [${describeLocator(query.within)}]` : description;
}
export function describeExpectation(value?: OperationExpectation): string | undefined {
  if (!value) return undefined;
  if (value.kind === "unavailable") return `Expectation not retained: ${value.reason}`;
  if (value.kind === "value") return `${value.subject}: ${value.expected}`;
  const outcome = { visible: "to be visible", hidden: "to be absent", unique: "to resolve to one unambiguous match", actionable: "to be actionable" }[value.outcome];
  return `Expect ${describeLocator(value.query)} ${outcome}`;
}
