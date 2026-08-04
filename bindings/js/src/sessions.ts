import { resolveSession } from "./config.js";
import { NoSessionError } from "./errors.js";
import * as native from "./native.js";

export async function sessions(): Promise<string[]> {
  return native.sessions();
}

export async function closeAll(): Promise<void> {
  await native.closeAll();
}

export async function getRecording(session?: string): Promise<string> {
  const name = resolveSession(session);
  try {
    return await native.recording(name);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (message.includes(`no recording for session '${name}'`)) {
      throw new NoSessionError(`no recording for session '${name}'`);
    }
    throw error;
  }
}
