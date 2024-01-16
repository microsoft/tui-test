// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Powershell, columns: 80, rows: 15 });

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

  terminal.keyUp(1);
  await expect(terminal).toHaveValue("clear");
  expect(terminal.getCursor().x).toBe(7);
});

test("down arrow", async ({ terminal }) => {
  terminal.write("clear");
  await expect(terminal).toHaveValue("clear");

  terminal.write("\r");
  await expect(terminal).not.toHaveValue("clear");

  terminal.keyUp(1);
  await expect(terminal).toHaveValue("clear");
  expect(terminal.getCursor().x).toBe(7);

  terminal.keyDown(1);
  await expect(terminal).toHaveValue(">   ");
  expect(terminal.getCursor().x).toBe(2);
});
