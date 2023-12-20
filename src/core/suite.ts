import { TactTestOptions } from "../testing/option.js";
import { Shell } from "./shell.js";
import { Terminal } from "./term.js";

type SuiteType = "file" | "describe";

export type TestFunction = (args: { terminal: Terminal }) => void | Promise<void>;
export type Test = {
  title: string;
  testFunction: TestFunction;
  passed?: boolean;
  stdout?: string;
  stderr?: string;
  error?: string;
  errorStack?: string;
};

export class Suite {
  suites: Suite[] = [];
  tests: Test[] = [];
  results = [];
  readonly name: string;
  readonly type: SuiteType;

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
