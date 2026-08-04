export { ShellUse } from "./client.js";
export type {
  ExpectTextOptions,
  MouseButtonOptions,
  WaitTextOptions,
} from "./client.js";
export { uniqueSession } from "./ephemeral.js";
export { closeAll, getRecording, sessions } from "./sessions.js";
export {
  ExpectationError,
  InternalError,
  NoSessionError,
  ShellUseError,
  UsageError,
} from "./errors.js";
export type { ErrorKind } from "./errors.js";
export { VERSION } from "./version.js";
export type {
  ArtifactOptions,
  Cell,
  ClientOptions,
  Color,
  Cursor,
  OpenResult,
  Shell,
  Size,
  SpawnOptions,
  State,
  TerminalArtifact,
  Timeouts,
} from "./types.js";
