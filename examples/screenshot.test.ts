// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Powershell, rows: 10, columns: 40 });

test("take a screenshot", async ({ terminal }) => {
  terminal.write("foo");

  await expect(terminal).toHaveValue("foo");
  await expect(terminal).toMatchSnapshot();
});
