import { element, get, text } from "./dom.js";
import type { Attachment, ReportData } from "./types.js";

export class AttachmentsPanel {
  private readonly cache = new Map<string, { bytes: Uint8Array<ArrayBuffer>; mime: string; url: string }>();
  constructor(data: ReportData) {
    for (const file of data.attachments) {
      const row = element("div", "", "attachment");
      const preview = element("button", `${file.name} (${file.bytes.toLocaleString()} B)`);
      preview.dataset.attachment = file.name;
      preview.addEventListener("click", () => this.preview(file));
      const download = element("a", "Download");
      download.href = "#";
      download.download = file.name;
      download.dataset.download = file.name;
      download.addEventListener("click", () => { download.href = this.content(file).url; });
      row.append(preview, download);
      get("evidence-links").append(row);
    }
    for (const file of data.files.filter((file) => file.status !== "written")) {
      get("evidence-links").append(element("p", `${file.path}: ${file.status} (${file.reason || "not available"})`, "muted"));
    }
    text("recording-note", data.attachments.some((file) => file.name === "session.cast")
      ? "The complete included session.cast is embedded and downloadable for continuous replay in an asciicast player."
      : `Recording not included: ${data.details.recording?.reason || "inclusion is opt-in"}. Retained frame inspection does not require a recording.`);
  }
  private content(file: Attachment) {
    const existing = this.cache.get(file.name);
    if (existing) return existing;
    const bytes = Uint8Array.from(atob(file.data), (char) => char.charCodeAt(0));
    const mime = file.name.endsWith(".svg") ? "image/svg+xml" : file.name.endsWith(".json") ? "application/json" : "text/plain";
    const result = { bytes, mime, url: URL.createObjectURL(new Blob([bytes], { type: mime })) };
    this.cache.set(file.name, result);
    return result;
  }
  private preview(file: Attachment) {
    const content = this.content(file);
    const container = get("attachment-preview");
    container.replaceChildren(element("h2", file.name));
    if (content.mime === "image/svg+xml") {
      const image = element("img");
      image.src = content.url;
      image.alt = file.name;
      container.append(image);
    } else {
      const limit = 512 * 1024;
      container.append(element("pre", new TextDecoder().decode(content.bytes.subarray(0, limit), { stream: content.bytes.length > limit })));
      if (content.bytes.length > limit) container.append(element("p", "Preview limited to 512 KiB. Download contains the complete file.", "muted"));
    }
  }
}
