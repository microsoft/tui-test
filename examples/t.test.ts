// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Cmd });

test("", async ({ terminal }) => {
  terminal.write("foo");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("foo");
  await expect(terminal).toMatchSnapshot();
});

test("", async ({ terminal }) => {
  terminal.write("bar");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("bar");
  await expect(terminal).toMatchSnapshot();

  terminal.write("foo");
  await expect(terminal).toHaveValue("foo");
  await expect(terminal).toMatchSnapshot();
});
