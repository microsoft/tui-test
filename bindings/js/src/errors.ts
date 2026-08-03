import type { TerminalArtifact } from "./types.js";

export type ErrorKind =
  | "assertion"
  | "usage"
  | "no_session"
  | "daemon"
  | "version_mismatch"
  | "internal";

export class ShellUseError extends Error {
  readonly kind: ErrorKind;
  readonly exitCode: number;
  terminal?: TerminalArtifact;

  constructor(message: string, kind: ErrorKind = "internal", exitCode = 5) {
    super(message);
    this.name = new.target.name;
    this.kind = kind;
    this.exitCode = exitCode;
  }
}

export class ExpectationError extends ShellUseError {
  constructor(message: string) {
    super(message, "assertion", 1);
  }
}

export class UsageError extends ShellUseError {
  constructor(message: string) {
    super(message, "usage", 2);
  }
}

export class NoSessionError extends ShellUseError {
  constructor(message: string) {
    super(message, "no_session", 3);
  }
}

export class DaemonError extends ShellUseError {
  constructor(message: string) {
    super(message, "daemon", 4);
  }
}

export class VersionMismatchError extends ShellUseError {
  constructor(message: string) {
    super(message, "version_mismatch", 4);
  }
}

export class InternalError extends ShellUseError {
  constructor(message: string) {
    super(message, "internal", 5);
  }
}

export function makeError(kind: string | undefined, message: string): ShellUseError {
  switch (kind) {
    case "assertion":
      return new ExpectationError(message);
    case "usage":
      return new UsageError(message);
    case "no_session":
      return new NoSessionError(message);
    case "daemon":
      return new DaemonError(message);
    default:
      return new InternalError(message);
  }
}
