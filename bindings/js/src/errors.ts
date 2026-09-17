import type {
  FailureArtifactRef,
  FailureDetails,
} from "./types.js";

export type ErrorKind = "assertion" | "usage" | "no_session" | "internal";

export class TuiTestError extends Error {
  readonly kind: ErrorKind;
  readonly exitCode: number;
  readonly details?: FailureDetails;
  readonly artifact?: FailureArtifactRef;

  constructor(message: string, kind: ErrorKind = "internal", exitCode = 5) {
    super(message);
    this.name = new.target.name;
    this.kind = kind;
    this.exitCode = exitCode;
  }
}

export class ExpectationError extends TuiTestError {
  constructor(message: string) {
    super(message, "assertion", 1);
  }
}

export class UsageError extends TuiTestError {
  constructor(message: string) {
    super(message, "usage", 2);
  }
}

export class NoSessionError extends TuiTestError {
  constructor(message: string) {
    super(message, "no_session", 3);
  }
}

export class InternalError extends TuiTestError {
  constructor(message: string) {
    super(message, "internal", 5);
  }
}

export function makeError(kind: ErrorKind, message: string): TuiTestError {
  switch (kind) {
    case "assertion":
      return new ExpectationError(message);
    case "usage":
      return new UsageError(message);
    case "no_session":
      return new NoSessionError(message);
    case "internal":
      return new InternalError(message);
    default:
      throw new TypeError(`unknown native error kind ${JSON.stringify(kind)}`);
  }
}
