// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

// source: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html

const ESC = "\u001B";
const CSI = "\u001B[";

const keyUp = CSI + "A";
const keyDown = CSI + "B";
const keyRight = CSI + "C";
const keyLeft = CSI + "D";
const keyBackspace = "\u007F";
const keyDelete = CSI + "3~";

export default { keyUp, keyDown, keyRight, keyLeft, ESC, keyBackspace, keyDelete };
