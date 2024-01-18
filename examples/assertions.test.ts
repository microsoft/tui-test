// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Powershell });

test("make a regex assertion", async ({ terminal }) => {
  terminal.write("dir | measure\r");

  await expect(terminal).toHaveValue(/Count\s*:\s*[0-9]{2}/m);
});

test("make a text assertion", async ({ terminal }) => {
  terminal.write("echo 1\r");

  await expect(terminal).toHaveValue("1");
});

test("make a negative text assertion", async ({ terminal }) => {
  terminal.write("echo 1\r");
  await expect(terminal).toHaveValue("1");

  terminal.write("clear\r");
  await expect(terminal).not.toHaveValue("clear");
});
