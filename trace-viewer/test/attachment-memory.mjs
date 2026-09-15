import { fixture, attachment } from "./fixture.mjs";
import { parseReport } from "../src/model.js";
import { createResources } from "../src/resources.js";

if (!global.gc) throw new Error("The allocation probe requires --expose-gc");
async function retainedBytes() {
  global.gc();
  await new Promise(setImmediate);
  global.gc();
  return process.memoryUsage().arrayBuffers;
}
const data = fixture();
data.timeline.frames = [];
data.attachments = [attachment("session.cast", "x".repeat(64 * 1024 * 1024))];
const source = JSON.stringify(data);
const baseline = await retainedBytes();
const model = parseReport(source);
const parsed = await retainedBytes() - baseline;
const resources = createResources(model);
const withResources = await retainedBytes() - baseline;
const exact = model.attachments[0].blob.size === 64 * 1024 * 1024
  && await model.attachments[0].blob.slice(-1).text() === "x";
resources.dispose();
const disposed = await retainedBytes() - baseline;
console.log(JSON.stringify({ parsed, withResources, disposed, exact }));
