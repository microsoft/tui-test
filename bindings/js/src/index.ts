export { ShellUse } from "./client.js";
export type {
  ExpectTextOptions,
  MouseButtonOptions,
  WaitTextOptions,
} from "./client.js";
export { uniqueSession } from "./ephemeral.js";
export {
  closeAll,
  daemonStatus,
  daemonStop,
  getRecording,
  sessions,
} from "./sessions.js";
export {
  DaemonError,
  ExpectationError,
  InternalError,
  NoSessionError,
  ShellUseError,
  UsageError,
  VersionMismatchError,
} from "./errors.js";
export type { ErrorKind } from "./errors.js";
export { VERSION } from "./version.js";
export type {
  ArtifactOptions,
  Cell,
  ClientOptions,
  Color,
  Cursor,
  DaemonStatus,
  HomeOptions,
  OpenResult,
  Shell,
  Size,
  SpawnOptions,
  State,
  TerminalArtifact,
  Timeouts,
} from "./types.js";
