# TUI Test

TUI Test is a framework for testing terminal applications. It provides a rich API for writing tests that interact with a terminal application across macOS, Linux, and Windows with a wide range of shells. It is built to be **fast**, **reliable**, and **easy to use**.

## Installation

Install TUI Test as using `npm`:

```sh
npm i -D @microsoft/tui-test
```

or `yarn`

```sh
yarn add --dev @microsoft/tui-test
```

or `pnpm`

```sh
pnpm add -D @microsoft/tui-test
```

## Running Tests

Running tests is as simple as running TUI Test from the command line after installation:

```sh
tui-test
```

Or run it using `npx`:

```sh
npx @microsoft/tui-test
```


## Capabilities

### Resilient • No flaky tests

**Auto-wait** TUI Test provides a rich API for interacting with the terminal. It waits for the terminal to be ready before executing commands, and it provides tooling for waiting for terminal renders before executing assertions.

**Tracing**. Configure test retry strategy, capture stdout & stderr, and create detailed terminal snapshots to eliminate flakes.

### Full isolation • Fast execution

**Terminal contexts**. TUI Test creates a new 'terminal context' for each test, which includes a new terminal and new underlying pty. This delivers full test isolation with zero overhead. Creating new terminal contexts only takes a handful of milliseconds.

### Multi-platform / Multi-shell • No more "it works in my shell"

**Multi-platform**. TUI Test supports testing on macOS, Linux, and Windows with a wide range of shells when installed: `cmd`, `windows powershell`, `powershell`, `bash`, `git-bash`, `fish`, and `zsh`.

**Wide-support**. TUI Test uses [xterm.js](https://xtermjs.org/) to render the terminal, which is a widely used terminal emulator in projects like [VSCode](https://github.com/microsoft/vscode) and [Hyper](https://github.com/vercel/hyper).

## Examples

To find more TUI Test examples [check out the examples folder](./examples) or in TUI Test's [e2e tests](./test/).

### Terminal Program

This code snippet shows how to start the terminal with a specific program running.

```ts
import { test, expect } from "@microsoft/tui-test";

test.use({ program: { file: "git" } });

test("git shows usage message", async ({ terminal }) => {
  await expect(terminal.getByText("usage: git", { full: true })).toBeVisible();
});
```

### Terminal Screenshot

This code snippet shows how to take a screenshot of the terminal.

```ts
import { test, expect } from "@microsoft/tui-test";

test("take a screenshot", async ({ terminal }) => {
  terminal.write("foo")

  await expect(terminal.getByText("foo")).toBeVisible();
  await expect(terminal).toMatchSnapshot();
});
```

### Terminal Assertions

This code snippet shows how to use rich assertions of the terminal.

```ts
import { test, expect } from "@microsoft/tui-test";

test("make a regex assertion", async ({ terminal }) => {
  terminal.write("ls -l\r")

  await expect(terminal.getByText(/total [0-9]{3}/m)).toBeVisible();
});
```

## Contributing

This project welcomes contributions and suggestions. Most contributions require you to agree to a
Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us
the rights to use your contribution. For details, visit https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA bot will automatically determine whether you need to provide
a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions
provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft
trademarks or logos is subject to and must follow
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship.
Any use of third-party trademarks or logos are subject to those third-party's policies.

