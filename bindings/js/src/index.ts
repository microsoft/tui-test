export { TuiTest } from "./client.js";
export type {
  ClipboardWaitOptions,
  LocatorClickOptions,
  LocatorDirection,
  LocatorExpectOptions,
  LocatorHighlightOptions,
  LocatorWaitOptions,
  Locator,
  RelativeStyleSelectorOptions,
  RelativeTextSelectorOptions,
  StyleSelectorOptions,
  TextSelectorOptions,
  TextStyleExpectation,
  MouseButton,
  MouseButtonOptions,
  MouseClickOptions,
  TitleOptions,
  RecordingFormat,
  RecordingOptions,
  RestartOptions,
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
  AutomaticRecording,
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
