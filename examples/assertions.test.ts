// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tui-test";

test.use({ shell: Shell.Powershell });

test("make a regex assertion", async ({ terminal }) => {
  terminal.submit("dir | measure");

  await expect(terminal.getByText(/Count\s*:\s*[0-9]{2}/g)).toBeVisible();
});

test("make a text assertion", async ({ terminal }) => {
  terminal.submit("echo 1");

  await expect(terminal.getByText("1")).toBeVisible();
});

test("make a negative text assertion", async ({ terminal }) => {
  terminal.submit("echo 1");
  await expect(terminal.getByText("1")).toBeVisible();

  terminal.submit("clear");
  await expect(terminal.getByText("clear")).not.toBeVisible();
});
