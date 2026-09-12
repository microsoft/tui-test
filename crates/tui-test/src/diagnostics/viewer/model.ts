import type { CellMismatch, Frame, Operation, ReportData, Selection } from "./types.js";

export function cellAt(frame: Frame | undefined, x: number, y: number) {
  const id = frame?.grid[y]?.[x];
  return id === undefined ? undefined : frame?.cells[id];
}
export class TraceModel {
  readonly frames: readonly Frame[];
  readonly operations: readonly Operation[];
  readonly frameIndex: ReadonlyMap<number, number>;
  readonly failureOperation?: Operation;
  readonly failureIndex?: number;
  readonly failureMismatches: readonly CellMismatch[];
  readonly duration: number;
  selection: Selection = { index: -1, isFailure: false, sharesFailureFrame: false };
  constructor(readonly data: ReportData) {
    this.frames = data.timeline.frames;
    this.operations = data.details.recent_operations || [];
    this.frameIndex = new Map(this.frames.map((frame, index) => [frame.sequence, index]));
    this.failureIndex = this.frameIndex.get(data.timeline.failure_screen_sequence);
    this.failureOperation = this.operations.findLast((op) => op.result !== "ok" && op.screen_at_return === data.timeline.failure_screen_sequence);
    this.failureMismatches = (data.details.locator?.stages || [])
      .filter((stage) => stage.stage_index === data.details.locator?.failure_stage).flatMap((stage) =>
      (stage.mismatches || []).map((mismatch) => ({
        ...mismatch, stage: stage.stage_index, x: mismatch.location.column,
        y: mismatch.location.row - (data.details.locator?.viewport_origin_y || 0),
      })));
    this.duration = Math.max(0, ...this.operations.map((op) => op.ended_ms), ...this.frames.map((frame) => frame.last_seen_ms));
  }
  selectFrame(index?: number, operation?: Operation, missingSequence?: number) {
    const frame = index === undefined ? undefined : this.frames[index];
    const sharesFailureFrame = frame !== undefined && index === this.failureIndex;
    this.selection = {
      index: frame ? index! : -1, frame, operation, missingSequence,
      sharesFailureFrame, isFailure: sharesFailureFrame && (!operation || operation === this.failureOperation),
    };
  }
  selectAction(operation: Operation) {
    this.selectFrame(this.frameIndex.get(operation.screen_at_return), operation, operation.screen_at_return);
  }
  selectFailure() {
    this.selectFrame(this.failureIndex, this.failureOperation, this.data.timeline.failure_screen_sequence);
  }
  mismatches(): readonly CellMismatch[] {
    return this.selection.isFailure ? this.failureMismatches : [];
  }
}
