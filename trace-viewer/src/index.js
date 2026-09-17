import "../style.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { parseReport } from "./model.js";
import { createResources } from "./resources.js";

/** @param {string} message */
function showError(message) {
  const error = document.getElementById("report-error");
  if (!error) throw new Error(message);
  error.textContent = `Report viewer error: ${message}. Captured evidence remains in the embedded report-data block.`;
  error.hidden = false;
}

window.addEventListener("error", (event) => showError(event.message));
window.addEventListener("unhandledrejection", (event) => showError(String(event.reason)));
try {
  const source = document.getElementById("report-data")?.textContent;
  if (!source) throw new Error("Missing embedded report-data");
  const target = document.getElementById("workbench");
  if (!target) throw new Error("Missing workbench root");
  const model = parseReport(source);
  const resources = createResources(model);
  try {
    mount(App, { target, props: { model, resources, hash: location.hash } });
  } catch (error) {
    resources.dispose();
    throw error;
  }
} catch (error) {
  showError(error instanceof Error ? error.message : String(error));
}
