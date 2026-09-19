import type {
  FailureCellMismatch, FailureReport, FailureOperationEvent, FailureScreenSnapshot,
} from "./report.js";

export type Operation = FailureOperationEvent;
export interface FrameCell {
  readonly char: string;
  readonly width: number;
  readonly fg: string | number;
  readonly bg: string | number;
  readonly underline_color: string | number;
  readonly underline_style: string;
  readonly flags: readonly string[];
  readonly resolved_fg: string;
  readonly resolved_bg: string;
  readonly resolved_underline_color: string;
  readonly link?: string;
  readonly link_id?: string;
}
export interface Frame extends FailureScreenSnapshot {
  readonly cells: readonly FrameCell[];
  readonly grid: readonly (readonly number[])[];
  readonly svg?: string;
  readonly omission?: string;
}
export interface Geometry {
  readonly grid_x: number;
  readonly grid_y: number;
  readonly cell_width: number;
  readonly cell_height: number;
}
export interface Attachment {
  readonly name: string;
  readonly bytes: number;
  readonly data: string;
}
export interface ReportData {
  readonly details: FailureReport;
  readonly timeline: {
    readonly schema_version?: number;
    readonly frames: readonly Frame[];
    readonly geometry: Geometry;
    readonly failure_screen_sequence: number;
  };
  readonly explanation: { readonly title: string; readonly expected: string; readonly actual: string; readonly note: string };
  readonly files: readonly { readonly path: string; readonly status: string; readonly reason?: string }[];
  readonly errors: readonly string[];
  readonly attachments: readonly Attachment[];
}
export interface CellMismatch extends FailureCellMismatch {
  readonly stage: number;
  readonly x: number;
  readonly y: number;
}
export interface ImageResult {
  readonly url?: string;
  readonly error?: string;
}

export type Properties = Readonly<Record<string, string | number | boolean | null | undefined>>;
export interface TraceFrame {
  readonly index: number;
  readonly sequence: number;
  readonly started: number;
  readonly ended: number;
  readonly columns: number;
  readonly rows: number;
  readonly width: number;
  readonly height: number;
  readonly viewBox: string;
  readonly cursor: { readonly x: number; readonly y: number };
  readonly cursorAppearance: string;
  readonly text: string;
  readonly svg?: string;
  readonly omission?: string;
  readonly cells: readonly FrameCell[];
  readonly grid: readonly (readonly number[])[];
  readonly label: string;
  readonly description: string;
  readonly evidence: unknown;
}
export interface TraceAction {
  readonly id: number;
  readonly screen: number;
  readonly frameIndex?: number;
  readonly name: string;
  readonly label: string;
  readonly locator?: string;
  readonly expectation?: string;
  readonly unavailableExpectation: boolean;
  readonly code: boolean;
  readonly searchText: string;
  readonly failed: boolean;
  readonly assertion: boolean;
  readonly started: number;
  readonly ended: number;
  readonly duration: number;
  readonly result: string;
  readonly properties: Properties;
  readonly pointer?: { readonly x: number; readonly y: number };
  readonly evidence: unknown;
}
export interface TraceAttachment {
  readonly id: number;
  readonly name: string;
  readonly byteLength: number;
  readonly blob: Blob;
  readonly mime: string;
  readonly preview: string;
  readonly truncated: boolean;
}
export interface TraceModel {
  readonly title: string;
  readonly hasFailure: boolean;
  readonly duration: number;
  readonly frames: readonly TraceFrame[];
  readonly actions: readonly TraceAction[];
  readonly geometry: Geometry;
  readonly failureFrameIndex?: number;
  readonly failureActionId?: number;
  readonly failureScreen: number;
  readonly mismatches: readonly CellMismatch[];
  readonly summary: Properties;
  readonly metadata: {
    readonly properties: Properties;
    readonly timeouts: Properties;
    readonly retention: string;
    readonly evidence: string;
  };
  readonly result: {
    readonly title: string;
    readonly expected: string;
    readonly actual: string;
    readonly note: string;
    readonly timing: string;
    readonly locator?: string;
    readonly hints: readonly string[];
    readonly error: string;
    readonly evidence: string;
  };
  readonly attachments: readonly TraceAttachment[];
  readonly unavailableFiles: readonly string[];
  readonly recordingNote: string;
}
export type SidebarTab = "actions" | "metadata";
export type DetailsTab = "errors" | "cell" | "call" | "attachments";
export type Tab = SidebarTab | DetailsTab;
export interface ViewerState {
  readonly frameIndex?: number;
  readonly actionId?: number;
  readonly missingScreen?: number;
  readonly sidebarTab: SidebarTab;
  readonly detailsTab: DetailsTab;
  readonly filter: string;
  readonly assertionsOnly: boolean;
  readonly column: string;
  readonly row: string;
  readonly cell?: { readonly x: number; readonly y: number };
  readonly zoom: "fit" | "1" | "1.5";
  readonly playback?: { readonly target: number; readonly actionId?: number };
  readonly attachmentId?: number;
  readonly failedImages: readonly number[];
  readonly failedAttachments: readonly number[];
}
export type ViewerEvent =
  | { readonly type: "select-frame"; readonly index: number }
  | { readonly type: "select-action"; readonly id: number }
  | { readonly type: "jump-to-result" }
  | { readonly type: "navigate-action"; readonly delta: -1 | 1 }
  | { readonly type: "select-tab"; readonly tab: Tab }
  | { readonly type: "filter"; readonly value: string }
  | { readonly type: "assertions-only"; readonly value: boolean }
  | { readonly type: "coordinate"; readonly axis: "column" | "row"; readonly value: string }
  | { readonly type: "inspect"; readonly x: number; readonly y: number; readonly activate: boolean }
  | { readonly type: "move-cell"; readonly dx: number; readonly dy: number }
  | { readonly type: "zoom"; readonly value: ViewerState["zoom"] }
  | { readonly type: "preview-attachment"; readonly id: number }
  | { readonly type: "image-failed"; readonly index: number }
  | { readonly type: "attachment-image-failed"; readonly id: number }
  | { readonly type: "toggle-playback" | "pause" | "tick" };
export interface Resources {
  readonly images: readonly ImageResult[];
  readonly attachments: readonly string[];
  readonly dispose: () => void;
}
export type CellInspection = ReturnType<typeof import("./model.js").inspectCell>;
