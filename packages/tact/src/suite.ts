import { TactTestOptions } from "./test/option.js";
import { Terminal } from "./terminal/term.js";
import { TactTestConfig, TactProjectConfig } from "./config/config.js";
import { glob } from "glob";

type SuiteType = "file" | "describe" | "project";

export const loadSuites = async (config: Required<TactTestConfig>): Promise<Suite[]> => {
  const projects: Required<TactProjectConfig>[] = [
    { shell: config.use.shell!, rows: config.use.rows!, columns: config.use.columns!, testMatch: config.testMatch!, name: "" },
    ...(config.projects?.map((project) => ({
      shell: project.shell ?? config.use.shell!,
      name: project.name ?? "",
      rows: project.rows ?? config.use.rows!,
      columns: project.columns ?? config.use.columns!,
      testMatch: project.testMatch,
    })) ?? []),
  ];

  return (
    await Promise.all(
      projects.map(async (project) => {
        const files = await glob(project.testMatch, { ignore: ["node_modules/**"] });
        const suite = new Suite(project.name, "project", { shell: project.shell, rows: project.rows, columns: project.columns });
        suite.suites = files.map((file) => new Suite(file, "file", { shell: project.shell, rows: project.rows, columns: project.columns }, suite));
        return suite;
      })
    )
  ).flat();
};

export type TestFunction = (args: { terminal: Terminal }) => void | Promise<void>;
export type Test = {
  title: string;
  testFunction: TestFunction;
  suiteId: number;
  globalId: string;
  results: TestResult[];
  callsite?: { line: number; column: number };
};
type TestResult = {
  passed: boolean;
  error?: string;
  executionTime: number;
};

export type TestMap = {
  [id: number]: Test;
};

export class Suite {
  suites: Suite[] = [];
  tests: Test[] = [];
  readonly name: string;
  readonly projectName: string | undefined;
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
