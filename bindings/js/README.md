# @microsoft/tui-test

Control, inspect, test, and record terminal apps from JavaScript or TypeScript. Sessions run the Rust engine in your process without requiring the `tui-test` CLI.

Requires Node.js 20 or newer. Keep optional dependencies enabled so npm installs the prebuilt native library for your platform. The package is ESM only.

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

See the [JavaScript and TypeScript API reference](https://tui-test.dev/docs/api/javascript/) for all methods, test helpers, recordings, and platform details, or start with [writing and running tests](https://tui-test.dev/docs/getting-started/testing/) and [locators](https://tui-test.dev/docs/guides/locators/). For local source builds, see [building from source](https://tui-test.dev/docs/contributing/#build-from-source).
