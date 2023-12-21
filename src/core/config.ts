import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import type { TactTestConfig } from "../testing/types.js";
import { defaultShell } from "./shell.js";

const configPath = path.join(process.cwd(), ".tact", "cache", "tact.config.js");
let loadedConfig: Required<TactTestConfig> | undefined;

export const loadConfig = async (): Promise<Required<TactTestConfig>> => {
  const userConfig: TactTestConfig = !fs.existsSync(configPath) ? {} : (await import(`file://${configPath}`)).default;
  loadedConfig = {
    testMatch: userConfig.testMatch ?? "**/*.@(spec|test).?(c|m)[jt]s?(x)",
    expect: {
      timeout: userConfig.timeout ?? 5_000,
    },
    globalTimeout: userConfig.globalTimeout ?? 0,
    retries: userConfig.retries ?? 0,
    projects: userConfig.projects ?? [],
    timeout: userConfig.timeout ?? 30_000,
    reporter: userConfig.reporter ?? "list",
    use: {
      shell: userConfig.use?.shell ?? defaultShell,
      rows: userConfig.use?.rows ?? 30,
      columns: userConfig.use?.columns ?? 80,
    },
  };
  return loadedConfig;
};

export const getExpectTimeout = (): number => loadedConfig?.expect.timeout ?? 5_000;
export const getTimeout = (): number => loadedConfig?.timeout ?? 30_000;
