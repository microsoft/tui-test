import type { Response } from "./types.js";

export interface NativeSessionHandle {
  name(): string;
  request(payload: unknown): Promise<Response>;
}

interface NativeBinding {
  NativeSession: new (name: string) => NativeSessionHandle;
  sessions(): Promise<string[]>;
  closeAll(): Promise<void>;
  closeAllSync(): void;
  recording(name: string): Promise<string>;
}

let bindingPromise: Promise<NativeBinding> | undefined;
let cachedBinding: NativeBinding | undefined;
let exitHookInstalled = false;

function installExitHook(): void {
  if (exitHookInstalled) {
    return;
  }
  exitHookInstalled = true;
  process.once("exit", () => {
    try {
      cachedBinding?.closeAllSync();
    } catch {}
  });
}

async function importBinding(): Promise<NativeBinding> {
  try {
    const module = await import("../native/index.js");
    const binding = module as unknown as NativeBinding;
    cachedBinding = binding;
    installExitHook();
    return binding;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      "failed to load the @microsoft/shell-use native addon: " +
        `${message}. Build it with \`npm run build:native\` (requires a Rust ` +
        "toolchain), or install a matching prebuilt platform package.",
      { cause: error },
    );
  }
}

async function loadBinding(): Promise<NativeBinding> {
  if (!bindingPromise) {
    bindingPromise = importBinding();
  }
  try {
    return await bindingPromise;
  } catch (error) {
    bindingPromise = undefined;
    throw error;
  }
}

async function createSession(name: string): Promise<NativeSessionHandle> {
  const binding = await loadBinding();
  return new binding.NativeSession(name);
}

export class NativeRuntime {
  #session: Promise<NativeSessionHandle>;

  constructor(name: string) {
    this.#session = createSession(name);
  }

  async request(payload: unknown): Promise<Response> {
    const session = await this.#session;
    return session.request(payload);
  }
}

export async function sessions(): Promise<string[]> {
  const binding = await loadBinding();
  return binding.sessions();
}

export async function closeAll(): Promise<void> {
  const binding = await loadBinding();
  await binding.closeAll();
}

export async function recording(name: string): Promise<string> {
  const binding = await loadBinding();
  return binding.recording(name);
}
