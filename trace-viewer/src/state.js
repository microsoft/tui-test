import { cellAt, selection, visibleActions } from "./model.js";

/** @import { TraceModel, ViewerState, ViewerEvent } from "./types.js" */
export const PLAYBACK_INTERVAL = 400;

/** @param {TraceModel} model @param {string} hash @returns {ViewerState} */
export function initialState(model, hash = "") {
  /** @type {ViewerState} */
  const state = {
    sidebarTab: "actions", detailsTab: "errors", filter: "", assertionsOnly: false,
    column: "0", row: "0", zoom: "fit", failedImages: [], failedAttachments: [],
  };
  const link = /^#screen-(\d+)$/.exec(hash);
  const index = link ? model.frames.findIndex((frame) => frame.sequence === Number(link[1])) : -1;
  return index >= 0 ? select(model, state, index) : select(model, state, model.failureFrameIndex, model.failureActionId, model.failureScreen);
}

/** @param {TraceModel} model @param {ViewerState} state @param {number | undefined} frameIndex
 * @param {number} [actionId] @param {number} [missingScreen] @returns {ViewerState}
 */
function select(model, state, frameIndex, actionId, missingScreen) {
  const next = { ...state, frameIndex, actionId, missingScreen, playback: undefined };
  const { frame, action, mismatches } = selection(model, next);
  const mismatch = mismatches.find((entry) => cellAt(frame, entry.x, entry.y));
  const point = mismatch ?? action?.pointer ?? (frame && {
    x: Math.min(frame.cursor.x, frame.columns - 1), y: Math.min(frame.cursor.y, frame.rows - 1),
  });
  return { ...next, column: String(point?.x ?? 0), row: String(point?.y ?? 0),
    cell: point && cellAt(frame, point.x, point.y) ? { x: point.x, y: point.y } : undefined };
}

/** @param {TraceModel} model @param {ViewerState} state @param {ViewerEvent} event @returns {ViewerState} */
export function update(model, state, event) {
  switch (event.type) {
    case "select-frame":
      return Number.isInteger(event.index) && model.frames[event.index] ? select(model, state, event.index) : state;
    case "select-action": {
      const action = model.actions.find((action) => action.id === event.id);
      if (!action) return state;
      const next = select(model, state, action.frameIndex, action.id, action.screen);
      return state.detailsTab === "errors" || state.detailsTab === "call"
        ? { ...next, detailsTab: selection(model, next).isFailure ? "errors" : "call" } : next;
    }
    case "jump-to-result":
      return { ...select(model, state, model.failureFrameIndex, model.failureActionId, model.failureScreen), detailsTab: "errors" };
    case "navigate-action": {
      const actions = visibleActions(model, state);
      const position = actions.findIndex((action) => action.id === state.actionId);
      const target = position < 0 ? (event.delta < 0 ? actions.length - 1 : 0) : position + event.delta;
      return actions[target] ? update(model, state, { type: "select-action", id: actions[target].id }) : state;
    }
    case "select-tab":
      return event.tab === "actions" || event.tab === "metadata"
        ? { ...state, sidebarTab: event.tab } : { ...state, detailsTab: event.tab };
    case "filter": return { ...state, filter: event.value, playback: undefined };
    case "assertions-only": return { ...state, assertionsOnly: event.value, playback: undefined };
    case "coordinate": return { ...state, [event.axis]: event.value };
    case "inspect": {
      const frame = selection(model, state).frame;
      const cell = cellAt(frame, event.x, event.y);
      return { ...state, playback: undefined, cell: cell ? { x: event.x, y: event.y } : undefined,
        column: Number.isFinite(event.x) ? String(event.x) : "",
        row: Number.isFinite(event.y) ? String(event.y) : "",
        detailsTab: event.activate ? "cell" : state.detailsTab };
    }
    case "move-cell": {
      const frame = selection(model, state).frame;
      if (!frame) return state;
      return update(model, state, { type: "inspect", activate: true,
        x: Math.max(0, Math.min(frame.columns - 1, (state.cell?.x ?? 0) + event.dx)),
        y: Math.max(0, Math.min(frame.rows - 1, (state.cell?.y ?? 0) + event.dy)) });
    }
    case "zoom": return { ...state, zoom: event.value };
    case "preview-attachment": return model.attachments[event.id] ? { ...state, attachmentId: event.id } : state;
    case "image-failed": return state.failedImages.includes(event.index) ? state
      : { ...state, failedImages: [...state.failedImages, event.index] };
    case "attachment-image-failed": return state.failedAttachments.includes(event.id) ? state
      : { ...state, failedAttachments: [...state.failedAttachments, event.id] };
    case "pause": return state.playback ? { ...state, playback: undefined } : state;
    case "toggle-playback": {
      if (state.playback) return { ...state, playback: undefined };
      const index = state.frameIndex;
      if (index === undefined || index >= model.frames.length - 1) return state;
      const action = visibleActions(model, state).find((action) =>
        (action.assertion || action.failed) && action.frameIndex !== undefined && action.frameIndex > index);
      return { ...state, playback: { target: action?.frameIndex ?? model.frames.length - 1, actionId: action?.id } };
    }
    case "tick": {
      if (!state.playback || state.frameIndex === undefined) return state;
      const { target, actionId } = state.playback;
      const index = Math.min(state.frameIndex + 1, target);
      if (index >= target && actionId !== undefined) return update(model, state, { type: "select-action", id: actionId });
      const next = select(model, state, index);
      return { ...next, playback: index >= target ? undefined : state.playback };
    }
  }
}
