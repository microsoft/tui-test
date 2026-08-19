import { resolveSession } from "./config.js";
import * as native from "./native.js";

export async function sessions(): Promise<string[]> {
  return native.sessions();
}

export async function closeAll(): Promise<void> {
  await native.closeAll();
}

export async function getRecording(session?: string): Promise<string> {
  return native.recording(resolveSession(session));
}
