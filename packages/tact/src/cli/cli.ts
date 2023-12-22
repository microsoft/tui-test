#!/usr/bin/env node

import { run } from "../runner/runner.js";

setImmediate(async () => await run());
