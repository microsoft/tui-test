import { TactTestOptions } from "../testing/option.js";
import { Terminal } from "./term.js";
import type { TactTestConfig, TactProjectConfig } from "../testing/types.js";
import { glob } from "glob";

type SuiteType = "file" | "describe" | "project";

export const loadSuites = async (config: Required<TactTestConfig>): Promise<Suite[]> => {
  const projects: Array<Required<TactProjectConfig>> = [
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
