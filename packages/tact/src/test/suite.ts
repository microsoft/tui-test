import { glob } from "glob";

import { TactTestOptions } from "./option.js";
import { TactTestConfig, TactProjectConfig } from "../config/config.js";
import type { TestCase } from "./testcase.js";

type SuiteType = "file" | "describe" | "project" | "root";

export class Suite {
  suites: Suite[] = [];
  tests: TestCase[] = [];
  source?: string;

  constructor(readonly title: string, readonly type: SuiteType, public options?: TactTestOptions, public parentSuite?: Suite) {}

  allTests(): TestCase[] {
    const suitesIterable = [...this.suites];
    const tests = [];
    while (suitesIterable.length != 0) {
      const suite = suitesIterable.shift();
      tests.push(...(suite?.tests ?? []));
      suitesIterable.push(...(suite?.suites ?? []));
    }
    return tests;
  }

  titlePath(): string[] {
    const titles = [];
    let currentSuite = this.parentSuite;
    while (currentSuite != null) {
      if (currentSuite.type === "project") {
        titles.push(`[${currentSuite.title}]`);
      } else if (currentSuite.type !== "root") {
        titles.push(currentSuite.title);
      }
      currentSuite = currentSuite.parentSuite;
    }
    return [...titles.reverse(), this.title];
  }
}

export const getRootSuite = async (config: Required<TactTestConfig>): Promise<Suite> => {
  const projects: Required<TactProjectConfig>[] = [
    { shell: config.use.shell!, rows: config.use.rows!, columns: config.use.columns!, testMatch: config.testMatch!, name: "", env: config.use.env! },
    ...(config.projects?.map((project) => ({
      shell: project.shell ?? config.use.shell!,
      name: project.name ?? "",
      rows: project.rows ?? config.use.rows!,
      columns: project.columns ?? config.use.columns!,
      testMatch: project.testMatch,
      env: project.env ?? config.use.env!,
    })) ?? []),
  ];

  const suites = (
    await Promise.all(
      projects.map(async (project) => {
        const files = await glob(project.testMatch, { ignore: ["**/node_modules/**"] });
        const suite = new Suite(project.name, "project", { shell: project.shell, rows: project.rows, columns: project.columns });
        suite.suites = files.map((file) => new Suite(file, "file", { shell: project.shell, rows: project.rows, columns: project.columns }, suite));
        return suite;
      })
    )
  ).flat();

  const rootSuite = new Suite("Root Suite", "root", { shell: config.use.shell!, rows: config.use.rows!, columns: config.use.columns! });
  rootSuite.suites = suites;
  return rootSuite;
};
