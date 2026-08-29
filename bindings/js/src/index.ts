export { TuiTest } from "./client.js";
export type {
  LocatorClickOptions,
  LocatorDirection,
  LocatorExpectOptions,
  LocatorHighlightOptions,
  LocatorWaitOptions,
  Locator,
  MouseButtonOptions,
  RelativeStyleSelectorOptions,
  RelativeTextSelectorOptions,
  StyleSelectorOptions,
  TextSelectorOptions,
  TextStyleExpectation,
  TitleOptions,
  RecordingFormat,
  RecordingOptions,
  ScreenshotOptions,
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
  BellEvent,
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
  TextMatch,
  TerminalArtifact,
  Timeouts,
} from "./types.js";
