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

test.describe("key controls", () => {
  test("left key", async ({ terminal }) => {
    terminal.write("bar");
    terminal.keyLeft(3);
    terminal.write("foo");

    await expect(terminal).toHaveValue("foobar");
    expect(terminal.getCursor().x).toBe(5);
  });

  test("right key", async ({ terminal }) => {
    terminal.write("bar");
    terminal.keyLeft(3);
    terminal.write("foo");
    terminal.keyRight(3);
    terminal.write("baz");

    await expect(terminal).toHaveValue("foobarbaz");
    expect(terminal.getCursor().x).toBe(11);
  });

  test("up arrow", async ({ terminal }) => {
    terminal.write("clear");
    await expect(terminal).toHaveValue("clear");

    terminal.write("\r");
    await expect(terminal).not.toHaveValue("clear");

    terminal.keyUp();
    await expect(terminal).toHaveValue("clear");
    expect(terminal.getCursor().x).toBe(7);
  });

  test("down arrow", async ({ terminal }) => {
    terminal.write("clear");
    await expect(terminal).toHaveValue("clear");

    terminal.write("\r");
    await expect(terminal).not.toHaveValue("clear");

    terminal.keyUp();
    await expect(terminal).toHaveValue("clear");
    expect(terminal.getCursor().x).toBe(7);

    terminal.keyDown();
    await expect(terminal).not.toHaveValue("clear");
    expect(terminal.getCursor().x).toBe(2);
  });

  test.when(isNotMacOS, "ctrl+c", async ({ terminal }) => {
    terminal.keyCtrlC();
    await expect(terminal).toHaveValue("^C");
  });

  test.when(isWindows, "ctrl+d", async ({ terminal }) => {
    terminal.keyCtrlD();
    await expect(terminal).toHaveValue("^D");
  });

  test("backspace", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal).toHaveValue("foo");

    terminal.keyBackspace(3);
    await expect(terminal).toHaveValue(">   ");
  });

  test("delete", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal).toHaveValue("foo");

    terminal.keyLeft(3);
    terminal.keyDelete(3);
    await expect(terminal).toHaveValue(">   ");
  });
});

test.describe("terminal functions", () => {
  test("resize", async ({ terminal }) => {
    terminal.write("foo");
    await expect(terminal).toHaveValue("foo");
    await expect(terminal).toMatchSnapshot();

    terminal.resize(40, 10);
    terminal.write("bar");
    await expect(terminal).toHaveValue("foobar");
    await expect(terminal).toMatchSnapshot();
  });

  test("cursor position", async ({ terminal }) => {
    terminal.resize(40, 10);

    expect(terminal.getCursor().x).toBe(2);
    expect(terminal.getCursor().y).toBe(0);
    expect(terminal.getCursor().baseY).toBe(0);

    terminal.write("echo foo\r");
    terminal.write("echo bar\r");
    await expect(terminal).toHaveValue("bar");
    await expect(terminal).toHaveValue(">  ");
    expect(terminal.getCursor().x).toBe(2);
    expect(terminal.getCursor().y).toBeGreaterThanOrEqual(4);
    expect(terminal.getCursor().baseY).toBe(0);

    for (let i = 0; i < 20; i++) {
      terminal.write(`echo ${i}\r`);
    }
    await expect(terminal).toHaveValue("19");
    await expect(terminal).toHaveValue(">  ");
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
      await expect(terminal).toHaveValue(">  ");
      await expect(terminal).not.toHaveValue("Microsoft Corporation");
    });
  });

  test.describe("cmd", () => {
    test.use({ shell: Shell.Cmd });
    test.when(isWindows, "has logo", async ({ terminal }) => {
      await expect(terminal).toHaveValue("Microsoft Corporation");
    });
  });

  test.describe("program", () => {
    test.use({ program: { file: "git" } });
    test("git shows usage message", async ({ terminal }) => {
      await expect(terminal).toHaveValue("usage: git", { full: true });
    });
  });

  test.describe("program with args", () => {
    test.use({ program: { file: "git", args: ["status"] } });
    test("git shows status message", async ({ terminal }) => {
      await expect(terminal).toHaveValue("On branch", { full: true });
    });
  });
});
