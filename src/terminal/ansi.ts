// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

// source: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html

const ESC = "\u001B";
const CSI = "\u001B[";
const SEP = ";";

const keyUp = CSI + "A";
const keyDown = CSI + "B";
const keyRight = CSI + "C";
const keyLeft = CSI + "D";
const keyBackspace = "\u007F";
const keyDelete = CSI + "3~";
const keyCtrlC = String.fromCharCode(3);
const keyCtrlD = String.fromCharCode(4);
const saveScreen = CSI + "?47h";
const restoreScreen = CSI + "?47l";
const clearScreen = CSI + "2J";
const cursorTo = (x: number, y: number) => {
  return CSI + (y + 1) + SEP + (x + 1) + "H";
};

export enum MouseButton {
  Left = 0,
  Middle = 1,
  Right = 2,
}

const mouseDown = (x: number, y: number, button: MouseButton) => {
  return CSI + "<" + button + SEP + (x + 1) + SEP + (y + 1) + "M";
};
const mouseUp = (x: number, y: number, button: MouseButton) => {
  return CSI + "<" + button + SEP + (x + 1) + SEP + (y + 1) + "m";
};
const mouseMove = (x: number, y: number) => {
  return CSI + "<" + 35 + SEP + (x + 1) + SEP + (y + 1) + "M";
};

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
  saveScreen,
  restoreScreen,
  clearScreen,
  cursorTo,
  mouseDown,
  mouseUp,
  mouseMove,
};
