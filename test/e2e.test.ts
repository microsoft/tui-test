// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tui-test";
import os from "node:os";

const shell =
  os.platform() == "darwin"
    ? Shell.Zsh
    : os.platform() == "linux"
      ? Shell.Bash
      : Shell.Powershell;

test.use({ shell: shell, columns: 80, rows: 15 });

const isWindows = os.platform() == "win32";
const isNotMacOS = os.platform() != "darwin";
const isLinux = os.platform() == "linux";

const windowsShells = [
  Shell.Cmd,
  Shell.Powershell,
  Shell.WindowsPowershell,
  Shell.Xonsh,
];
const unixShells = [Shell.Bash, Shell.Fish, Shell.Zsh];

test.describe("locators", () => {
  test.describe("getByText", () => {
    test("string driven", async ({ terminal }) => {
      await expect(terminal.getByText(">")).toBeVisible();
    });

    test("regex driven", async ({ terminal }) => {
      await expect(terminal.getByText(/>/g)).toBeVisible();
    });

    test.fail("regex driven non-global regex", async ({ terminal }) => {
      await expect(terminal.getByText(/>/m)).toBeVisible();
    });
  });
});

test.describe("key controls", () => {
  test("left key", async ({ terminal }) => {
    terminal.write("bar");
    terminal.keyLeft(3);
    terminal.write("foo");

    await expect(terminal.getByText("foobar")).toBeVisible();
    expect(terminal.getCursor().x).toBe(5);
  });

  test("right key", async ({ terminal }) => {
    terminal.write("bar");
    terminal.keyLeft(3);
    terminal.write("foo");
    terminal.keyRight(3);
    terminal.write("baz");

    await expect(terminal.getByText("foobarbaz")).toBeVisible();
    expect(terminal.getCursor().x).toBe(11);
  });

  test("up arrow", async ({ terminal }) => {
    terminal.write("clear");
    await expect(terminal.getByText("clear")).toBeVisible();

    terminal.submit();
    await expect(terminal.getByText("clear")).not.toBeVisible();

    terminal.keyUp();
    await expect(terminal.getByText("clear")).toBeVisible();
    expect(terminal.getCursor().x).toBe(7);
  });

  test("down arrow", async ({ terminal }) => {
    terminal.write("clear");
    await expect(terminal.getByText("clear")).toBeVisible();

    terminal.submit();
    await expect(terminal.getByText("clear")).not.toBeVisible();

    terminal.keyUp();
    await expect(terminal.getByText("clear")).toBeVisible();
    expect(terminal.getCursor().x).toBe(7);

    terminal.keyDown();
    await expect(terminal.getByText("clear")).not.toBeVisible();
    expect(terminal.getCursor().x).toBe(2);
  });

  test.when(isNotMacOS, "ctrl+c", async ({ terminal }) => {
    await expect(terminal).toMatchSnapshot();
    terminal.keyCtrlC();

    await expect(terminal.getByText("^C")).toBeVisible();
    await expect(terminal).toMatchSnapshot();
  });

  test.when(isWindows, "ctrl+d", async ({ terminal }) => {
    terminal.keyCtrlD();
    await expect(terminal.getByText("^D")).toBeVisible();
  });

  test("backspace", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal.getByText("foo")).toBeVisible();

    terminal.keyBackspace(3);
    await expect(terminal.getByText("foo")).not.toBeVisible();
  });

  test("delete", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal.getByText("foo")).toBeVisible();

    terminal.keyLeft(3);
    terminal.keyDelete(3);
    await expect(terminal.getByText("foo")).not.toBeVisible();
  });
});

test.describe("terminal functions", () => {
  test("resize", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal.getByText("foo")).toBeVisible();
    await expect(terminal).toMatchSnapshot();

    terminal.resize(40, 10);
    terminal.write("bar");
    await expect(terminal.getByText("foobar")).toBeVisible();
    await expect(terminal).toMatchSnapshot();
  });

  test("cursor position", async ({ terminal }) => {
    terminal.resize(40, 10);

    expect(terminal.getCursor().x).toBe(2);
    expect(terminal.getCursor().y).toBe(0);
    expect(terminal.getCursor().baseY).toBe(0);

    terminal.submit("echo foo");
    terminal.submit("echo bar");
    await expect(terminal.getByText("bar", { strict: false })).toBeVisible();
    await expect(terminal.getByText(">  ")).toBeVisible();
    expect(terminal.getCursor().x).toBe(2);
    expect(terminal.getCursor().y).toBeGreaterThanOrEqual(4);
    expect(terminal.getCursor().baseY).toBe(0);

    for (let i = 0; i < 20; i++) {
      terminal.submit(`echo ${i}`);
    }
    await expect(terminal.getByText("19", { strict: false })).toBeVisible();
    await expect(terminal.getByText(">  ")).toBeVisible();
    expect(terminal.getCursor().x).toBe(2);
    expect(terminal.getCursor().y).toBe(9);

    expect(terminal.getCursor().baseY).toBeGreaterThanOrEqual(35);
  });
});

test.describe("test variations", () => {
  test.skip("skip", async () => {
    throw new Error("this must be skipped");
  });

  test.fail("fail", async () => {
    throw new Error("this must pass");
  });
});

test.describe("use variations", () => {
  test.describe("powershell", () => {
    test.use({ shell: Shell.Powershell });
    test.when(isWindows, "doesn't have logo", async ({ terminal }) => {
      await expect(terminal.getByText(">  ")).toBeVisible();
      await expect(
        terminal.getByText("Microsoft Corporation")
      ).not.toBeVisible();
    });
  });

  test.describe("cmd", () => {
    test.use({ shell: Shell.Cmd });
    test.when(isWindows, "has logo", async ({ terminal }) => {
      await expect(terminal.getByText("Microsoft Corporation")).toBeVisible();
    });
  });

  test.describe("program", () => {
    test.use({ program: { file: "git" } });
    test("git shows usage message", async ({ terminal }) => {
      await expect(
        terminal.getByText("usage: git", { full: true })
      ).not.toBeVisible();
    });
  });

  test.describe("program with args", () => {
    test.use({ program: { file: "git", args: ["status"] } });
    test("git shows status message", async ({ terminal }) => {
      await expect(
        terminal.getByText("On branch", { full: true })
      ).not.toBeVisible();
    });
  });
});

test.describe("locators", () => {
  test.describe("getByText", () => {
    test("match found", async ({ terminal }) => {
      await expect(terminal.getByText(">")).toHaveBgColor(0);
    });

    test.fail("match not found", async ({ terminal }) => {
      await expect(terminal.getByText("apple")).toHaveBgColor(0);
    });

    test.fail("strict mode failure", async ({ terminal }) => {
      terminal.submit("echo bar");
      terminal.write("foo");

      await expect(terminal.getByText("foo")).toBeVisible();
      await expect(terminal.getByText("bar")).toBeVisible();
    });

    test.fail("strict mode failure with .not", async ({ terminal }) => {
      terminal.submit("echo bar");
      terminal.write("foo");

      await expect(terminal.getByText("foo")).toBeVisible();
      await expect(terminal.getByText("bar")).not.toBeVisible();
    });
  });
});

test.describe("color detection", () => {
  test("checks the default background color", async ({ terminal }) => {
    await expect(terminal.getByText(">")).toHaveBgColor(0);
    await expect(terminal.getByText(">")).toHaveBgColor([0, 0, 0]);
  });

  test("checks the default foreground color", async ({ terminal }) => {
    await expect(terminal.getByText(">")).toHaveFgColor(0);
  });

  test.when(isLinux, "checks background color", async ({ terminal }) => {
    terminal.write(String.raw`printf "\x1b[42m%s%s\n\x1b[0m" foo bar`);
    terminal.submit();
    await expect(terminal.getByText("foobar")).toHaveBgColor(2);
  });

  test.when(isLinux, "checks foreground color", async ({ terminal }) => {
    terminal.write(String.raw`printf "\x1b[31m%s%s\n\x1b[0m" foo bar`);
    terminal.submit();
    await expect(terminal.getByText("foobar")).toHaveFgColor(1);
  });

  test.fail(
    "checks failure on background color when it doesn't match",
    async ({ terminal }) => {
      await expect(terminal.getByText(">")).toHaveBgColor(16);
    }
  );

  test.fail(
    "checks failure on background color when it matches .not",
    async ({ terminal }) => {
      await expect(terminal.getByText(">")).not.toHaveBgColor(0);
    }
  );
});

test.describe("shells", () => {
  const shells = isWindows ? windowsShells : unixShells;
  shells.forEach((shell) => {
    test.describe(`[${shell}]`, () => {
      test.use({ shell });

      test("simple controls", async ({ terminal }) => {
        terminal.submit("echo bar");
        await expect(
          terminal.getByText("bar", { strict: false })
        ).toBeVisible();

        await expect(terminal.getByText(">   ")).toBeVisible();

        terminal.write("tomato");
        await expect(terminal.getByText("tomato")).toBeVisible();
      });
    });
  });
});
