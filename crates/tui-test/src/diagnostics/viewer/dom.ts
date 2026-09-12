export function get(id: string): HTMLElement;
export function get<T extends Element>(id: string, constructor: new () => T): T;
export function get(id: string, constructor: new () => Element = HTMLElement): Element {
  const node = document.getElementById(id);
  if (!(node instanceof constructor)) throw new Error(`Missing or invalid viewer element: ${id}`);
  return node;
}
export const button = (id: string) => get(id, HTMLButtonElement);
export const input = (id: string) => get(id, HTMLInputElement);
export const text = (id: string, value: string) => { get(id).textContent = value; };
export const json = (value: unknown) => JSON.stringify(value, null, 2);
export const time = (ms?: number) => ms === undefined ? "Not captured" : `${ms} ms`;
export function element<K extends keyof HTMLElementTagNameMap>(tag: K, value = "", className = "") {
  const node = document.createElement(tag);
  node.textContent = value;
  node.className = className;
  return node;
}
export function showError(message: string) {
  text("report-error", `Report viewer error: ${message}. Captured evidence remains in the embedded report-data block.`);
  get("report-error").hidden = false;
}
export function pairs(id: string, values: Record<string, string | number | boolean | null | undefined>, colors: Record<string, string> = {}) {
  get(id).replaceChildren();
  for (const [key, value] of Object.entries(values)) {
    const content = element("dd", value == null ? "Not captured" : String(value));
    if (/^#[0-9a-f]{6}$/i.test(colors[key] || "")) {
      const swatch = element("span", "", "swatch");
      swatch.style.backgroundColor = colors[key];
      content.prepend(swatch);
    }
    get(id).append(element("dt", key), content);
  }
}
export class Tabs {
  private readonly buttons = Array.from(document.querySelectorAll<HTMLButtonElement>("[role=tab]"));
  constructor() {
    for (const tab of this.buttons) {
      tab.addEventListener("click", () => this.select(tab.id));
      tab.addEventListener("keydown", (event) => {
        const group = this.buttons.filter((other) => other.dataset.group === tab.dataset.group);
        const offsets: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, Home: -group.indexOf(tab), End: group.length - 1 - group.indexOf(tab) };
        const offset = offsets[event.key];
        if (offset === undefined) return;
        event.preventDefault();
        const next = group[(group.indexOf(tab) + offset + group.length) % group.length];
        this.select(next.id);
        next.focus();
      });
    }
  }
  select(id: string) {
    const selected = button(id);
    for (const tab of this.buttons.filter((tab) => tab.dataset.group === selected.dataset.group)) {
      const active = tab === selected;
      tab.setAttribute("aria-selected", String(active));
      tab.tabIndex = active ? 0 : -1;
      get(tab.getAttribute("aria-controls") || "").hidden = !active;
    }
  }
}
