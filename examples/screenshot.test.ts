// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tui-test";

test.use({ shell: Shell.Powershell, rows: 10, columns: 40 });

test("take a screenshot", async ({ terminal }) => {
  terminal.write("foo");

  await expect(terminal.getByText("foo")).toBeVisible();
  await expect(terminal).toMatchSnapshot();
});
