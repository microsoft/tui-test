// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Powershell, columns: 80, rows: 15 });

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

  await expect(terminal).toHaveValue("> ");
  expect(terminal.getCursor().x).toBe(2);
  expect(terminal.getCursor().y).toBe(0);
  expect(terminal.getCursor().baseY).toBe(0);

  terminal.write("echo foo\r");
  terminal.write("echo bar\r");
  await expect(terminal).toHaveValue("bar");
  await expect(terminal).toHaveValue(">  ");
  expect(terminal.getCursor().x).toBe(2);
  expect(terminal.getCursor().y).toBe(4);
  expect(terminal.getCursor().baseY).toBe(0);

  for (let i = 0; i < 20; i++) {
    terminal.write(`echo ${i}\r`);
  }
  await expect(terminal).toHaveValue("19");
  await expect(terminal).toHaveValue(">  ");
  expect(terminal.getCursor().x).toBe(2);
  expect(terminal.getCursor().y).toBe(9);
  expect(terminal.getCursor().baseY).toBe(35);
});
