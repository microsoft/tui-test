// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

// source: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html

const ESC = "\u001B";
const CSI = "\u001B[";

const keyUp = (count: number) => CSI + count + "A";
const keyDown = (count: number) => CSI + count + "B";
const keyRight = CSI + "C";
const keyLeft = CSI + "D";
const keyBackspace = "\u007F";
const keyDelete = CSI + "3~";
const keyCtrlC = String.fromCharCode(3);
const keyCtrlD = String.fromCharCode(4);

export default {
  keyUp,
  keyDown,
  keyRight,
  keyLeft,
  ESC,
  keyBackspace,
  keyDelete,
  keyCtrlC,
  keyCtrlD,
};
