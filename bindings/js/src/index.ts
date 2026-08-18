export { TuiTest } from "./client.js";
export type {
  ExpectTextOptions,
  MouseButtonOptions,
  TitleOptions,
  RecordingFormat,
  RecordingOptions,
  WaitTextOptions,
} from "./client.js";
export { uniqueSession } from "./ephemeral.js";
export { closeAll, getRecording, sessions } from "./sessions.js";
export {
  ExpectationError,
  InternalError,
  NoSessionError,
  TuiTestError,
  UsageError,
} from "./errors.js";
export type { ErrorKind } from "./errors.js";
export { VERSION } from "./version.js";
export type {
  ArtifactOptions,
  Backend,
  Cell,
  ClientOptions,
  Color,
  Colors,
  Cursor,
  EffectiveTimeouts,
  OpenResult,
  Profile,
  Shell,
  Size,
  SpawnOptions,
  State,
  TerminalArtifact,
  Timeouts,
} from "./types.js";
