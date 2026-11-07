# @microsoft/tui-test

Control, inspect, test, and record terminal apps from JavaScript or TypeScript.
The engine runs in your process; the CLI is not required.

Requires Node.js 20 or newer. Keep optional dependencies enabled so npm installs
the native libraries. The package is ESM only.

```sh
npm install @microsoft/tui-test
```

```js
import { TuiTest } from "@microsoft/tui-test";

const terminal = TuiTest.ephemeral();
try {
  await terminal.run(process.execPath, ["-e", "console.log('Ready')"]);
  await terminal.getByText("Ready").expect();
  await terminal.waitExit();
} finally {
  await terminal.closeQuiet();
}
```

See the [JavaScript and TypeScript API](https://tui-test.dev/docs/api/javascript/)
for all methods, options, platform requirements, and source builds.
Start with [writing tests](https://tui-test.dev/docs/getting-started/writing-tests/)
or the [locator guide](https://tui-test.dev/docs/guides/locators/).
