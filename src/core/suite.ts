import { TactTestOptions } from "../testing/option.js";
import { Shell } from "./shell.js";
import { Terminal } from "./term.js";

type SuiteType = "file" | "describe";

export type TestFunction = (args: { terminal: Terminal }) => void | Promise<void>;
export type Test = {
  title: string;
  testFunction: TestFunction;
  id: number;
  passed?: boolean;
  stdout?: string;
  stderr?: string;
  errorStack?: string;
};

export type TestMap = {
  [id: number]: Test;
};

export class Suite {
  suites: Suite[] = [];
  tests: Test[] = [];
  readonly name: string;
  readonly type: SuiteType;
  source?: string;

  constructor(name: string, type: SuiteType, private _options?: TactTestOptions, public parentSuite?: Suite) {
    this.name = name;
    this.type = type;
  }

  use(options: TactTestOptions) {
    this._options = { ...this._options, ...options };
  }

  options(): TactTestOptions {
    return this._options ?? {};
  }
}
