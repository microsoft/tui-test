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

export enum MouseKey {
  Left = 0,
  Middle = 1,
  Right = 2,
}

const mouseDown = (x: number, y: number, button: MouseKey) => {
  return CSI + "<" + button + SEP + (x + 1) + SEP + (y + 1) + "M";
};
const mouseUp = (x: number, y: number, button: MouseKey) => {
  return CSI + "<" + button + SEP + (x + 1) + SEP + (y + 1) + "m";
};
const mouseMove = (x: number, y: number) => {
  return CSI + "<" + 35 + SEP + (x + 1) + SEP + (y + 1) + "M";
};

export enum Key {
  Home = "Home",
  End = "End",
  PageUp = "PageUp",
  PageDown = "PageDown",
  Insert = "Insert",
  Delete = "Delete",
  Backspace = "Backspace",
  Tab = "Tab",
  Enter = "Enter",
  Space = "Space",
  Escape = "Escape",
  F1 = "F1",
  F2 = "F2",
  F3 = "F3",
  F4 = "F4",
  F5 = "F5",
  F6 = "F6",
  F7 = "F7",
  F8 = "F8",
  F9 = "F9",
  F10 = "F10",
  F11 = "F11",
  F12 = "F12",
}

type KeyModifiers = {
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
};

const modifierParam = (mods: KeyModifiers): number => {
  return 1 + (mods.shift ? 1 : 0) + (mods.alt ? 2 : 0) + (mods.ctrl ? 4 : 0);
};

const specialKeyMap: Record<
  Key,
  { seq: string; modified: (mod: number) => string }
> = {
  [Key.Home]: { seq: CSI + "H", modified: (m) => CSI + "1" + SEP + m + "H" },
  [Key.End]: { seq: CSI + "F", modified: (m) => CSI + "1" + SEP + m + "F" },
  [Key.PageUp]: { seq: CSI + "5~", modified: (m) => CSI + "5" + SEP + m + "~" },
  [Key.PageDown]: {
    seq: CSI + "6~",
    modified: (m) => CSI + "6" + SEP + m + "~",
  },
  [Key.Insert]: { seq: CSI + "2~", modified: (m) => CSI + "2" + SEP + m + "~" },
  [Key.Delete]: { seq: keyDelete, modified: (m) => CSI + "3" + SEP + m + "~" },
  [Key.Backspace]: {
    seq: keyBackspace,
    modified: (m) =>
      m === 1 ? keyBackspace : CSI + "27" + SEP + m + SEP + "127~",
  },
  [Key.Tab]: {
    seq: "\t",
    modified: (m) => (m === 1 ? "\t" : CSI + "27" + SEP + m + SEP + "9~"),
  },
  [Key.Enter]: {
    seq: "\r",
    modified: (m) => (m === 1 ? "\r" : CSI + "27" + SEP + m + SEP + "13~"),
  },
  [Key.Space]: {
    seq: " ",
    modified: (m) => (m === 1 ? " " : CSI + "27" + SEP + m + SEP + "32~"),
  },
  [Key.Escape]: {
    seq: ESC,
    modified: (m) => (m === 1 ? ESC : CSI + "27" + SEP + m + SEP + "27~"),
  },
  [Key.F1]: { seq: ESC + "OP", modified: (m) => CSI + "1" + SEP + m + "P" },
  [Key.F2]: { seq: ESC + "OQ", modified: (m) => CSI + "1" + SEP + m + "Q" },
  [Key.F3]: { seq: ESC + "OR", modified: (m) => CSI + "1" + SEP + m + "R" },
  [Key.F4]: { seq: ESC + "OS", modified: (m) => CSI + "1" + SEP + m + "S" },
  [Key.F5]: { seq: CSI + "15~", modified: (m) => CSI + "15" + SEP + m + "~" },
  [Key.F6]: { seq: CSI + "17~", modified: (m) => CSI + "17" + SEP + m + "~" },
  [Key.F7]: { seq: CSI + "18~", modified: (m) => CSI + "18" + SEP + m + "~" },
  [Key.F8]: { seq: CSI + "19~", modified: (m) => CSI + "19" + SEP + m + "~" },
  [Key.F9]: { seq: CSI + "20~", modified: (m) => CSI + "20" + SEP + m + "~" },
  [Key.F10]: { seq: CSI + "21~", modified: (m) => CSI + "21" + SEP + m + "~" },
  [Key.F11]: { seq: CSI + "23~", modified: (m) => CSI + "23" + SEP + m + "~" },
  [Key.F12]: { seq: CSI + "24~", modified: (m) => CSI + "24" + SEP + m + "~" },
};

const keyCombo = (key: string | Key, modifiers?: KeyModifiers): string => {
  const mods = modifiers ?? {};
  const hasModifier = mods.ctrl || mods.alt || mods.shift;

  if (Object.values(Key).includes(key as Key)) {
    const entry = specialKeyMap[key as Key];
    if (!hasModifier) {
      return entry.seq;
    }
    return entry.modified(modifierParam(mods));
  }

  if (key.length !== 1) {
    throw new Error(
      `Invalid key: '${key}'. Use the Key enum for special keys.`
    );
  }

  let char = key;

  if (mods.shift && /^[a-z]$/i.test(char)) {
    char = char.toUpperCase();
  }

  if (mods.ctrl) {
    const code = char.toUpperCase().charCodeAt(0);
    if (code >= 0x40 && code <= 0x5f) {
      const ctrlChar = String.fromCharCode(code - 0x40);
      // Alt+Ctrl: ESC prefix before the control character
      return mods.alt ? ESC + ctrlChar : ctrlChar;
    }
  }

  if (mods.alt) {
    return ESC + char;
  }

  return char;
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
  keyCombo,
};
