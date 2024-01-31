// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { test, expect } from "@microsoft/tui-test";

test.use({ program: { file: "git" } });

test("git shows usage message", async ({ terminal }) => {
  await expect(terminal).toHaveValue("usage: git", { full: true });
});
