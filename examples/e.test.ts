// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Cmd });

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("> ");
  expect(terminal.getCursor().x).toBe(1);
});

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("> ");
});
