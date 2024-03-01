// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { strictModeErrorPrefix } from "../utils/constants.js";

const getErrorMessage = (e: string | Error | unknown): string =>
  typeof e == "string" ? e : e instanceof Error ? e.message : "";

export const isStrictModeViolation = (error: string | Error | unknown) =>
  getErrorMessage(error).startsWith(strictModeErrorPrefix);
