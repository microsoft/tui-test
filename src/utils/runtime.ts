// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import process from "node:process";

declare const Bun: unknown;

export const isBun = (): boolean => typeof Bun !== "undefined";

export const isBunPtySupported = (): boolean =>
  isBun() && process.platform !== "win32";
