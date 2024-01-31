// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tui-test";

test.use({ shell: Shell.Cmd });

test("backspace", async ({ terminal }) => {
  terminal.write("foo");
  await expect(terminal).toHaveValue("foo");
  expect(terminal.getCursor().x).toBe(5);

  terminal.keyBackspace(3);
  await expect(terminal).not.toHaveValue("foo");
  expect(terminal.getCursor().x).toBe(2);
});

test("delete", async ({ terminal }) => {
  terminal.write("foo");
  await expect(terminal).toHaveValue("foo");
  expect(terminal.getCursor().x).toBe(5);

  terminal.keyBackspace(3);
  await expect(terminal).not.toHaveValue("foo");
  expect(terminal.getCursor().x).toBe(2);
});
