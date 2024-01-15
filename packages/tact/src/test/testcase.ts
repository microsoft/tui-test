// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { Terminal } from "../terminal/term.js";
import type { Suite } from "./suite.js";
import type { SnapshotStatus } from "./matchers/toMatchSnapshot.js";

export type Location = {
  row: number;
  column: number;
};

export type TestFunction = (args: {
  terminal: Terminal;
}) => void | Promise<void>;

export type TestStatus =
  | "expected"
  | "unexpected"
  | "pending"
  | "skipped"
  | "flaky";

export type TestResult = {
  status: TestStatus;
  error?: string;
  duration: number;
  snapshots: SnapshotStatus[];
};

export class TestCase {
  readonly id: string;
  readonly results: TestResult[] = [];
  constructor(
    readonly title: string,
    readonly location: Location,
    readonly testFunction: TestFunction,
    readonly suite: Suite,
    readonly expectedStatus: TestStatus = "expected"
  ) {
    this.id = this.titlePath().join("");
  }

  outcome(): TestStatus {
    if (
      this.results.length == 0 ||
      this.results.every((result) => result.status === "skipped")
    )
      return "skipped";
    let status = this.results[0].status;
    for (const result of this.results.slice(1)) {
      if (
        (status === "unexpected" && result.status === "expected") ||
        (status === "expected" && result.status !== "expected")
      ) {
        return "flaky";
      }
      status = result.status;
    }
    if (this.expectedStatus === status) return "expected";
    return status;
  }

  snapshots(): SnapshotStatus[] {
    return this.results.at(-1)?.snapshots ?? [];
  }

  filePath(): string | undefined {
    let currentSuite: Suite | undefined = this.suite;
    while (currentSuite != null) {
      if (currentSuite.type === "file") {
        return currentSuite.title;
      }
      currentSuite = currentSuite.parentSuite;
    }
  }

  titlePath(): string[] {
    const titles = [];
    let currentSuite: Suite | undefined = this.suite;
    while (currentSuite != null) {
      if (currentSuite.type === "project" && currentSuite.title.length != 0) {
        titles.push(`[${currentSuite.title}]`);
      } else if (currentSuite.type === "describe") {
        titles.push(currentSuite.title);
      } else if (currentSuite.type === "file") {
        titles.push(
          `${currentSuite.title}:${this.location.row}:${this.location.row}`
        );
      }
      currentSuite = currentSuite.parentSuite;
    }
    return [...titles.reverse(), this.title];
  }
}
