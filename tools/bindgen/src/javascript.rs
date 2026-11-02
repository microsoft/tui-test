//! JavaScript names and defaults are transport metadata, not duplicate Rust models.
use super::*;
use std::collections::BTreeSet;

// JavaScript method, owned operation, result variant.
const METHODS: &[(&str, &str, &str)] = &[
    ("restart", "Restart", "Open"),
    ("state", "State", "State"),
    ("text", "Text", "Text"),
    ("packedScreen", "PackedScreen", "PackedScreen"),
    ("cells", "Cells", "Cells"),
    ("findLocator", "FindLocator", "Matches"),
    ("waitLocator", "WaitLocator", "Unit"),
    ("expectLocator", "WaitLocator", "Unit"),
    ("clickLocator", "ClickLocator", "Unit"),
    ("highlightLocator", "HighlightLocator", "Matches"),
    ("getCommand", "GetCommand", "Command"),
    ("getOutput", "GetOutput", "Output"),
    ("getExitCode", "GetExitCode", "ExitCode"),
    ("getCwd", "GetCwd", "Cwd"),
    ("getTitle", "GetTitle", "Title"),
    ("getClipboard", "GetClipboard", "Clipboard"),
    ("getCursor", "GetCursor", "Cursor"),
    ("getSize", "GetSize", "Size"),
    ("getViewport", "GetViewport", "Viewport"),
    ("scroll", "Scroll.Lines", "Unit"),
    ("scrollToTop", "Scroll.Top", "Unit"),
    ("scrollToBottom", "Scroll.Bottom", "Unit"),
    ("getBellCount", "GetBellCount", "BellCount"),
    ("getBellEvents", "GetBellEvents", "BellEvents"),
    ("write", "Write", "Unit"),
    ("type", "Write", "Unit"),
    ("submit", "Submit", "Unit"),
    ("press", "Key.Press", "Unit"),
    ("keyDown", "Key.Down", "Unit"),
    ("repeat", "Key.Repeat", "Unit"),
    ("keyUp", "Key.Up", "Unit"),
    ("mouseClick", "Mouse.Click", "Unit"),
    ("mouseMove", "Mouse.Move", "Unit"),
    ("mouseDown", "Mouse.Down", "Unit"),
    ("mouseUp", "Mouse.Up", "Unit"),
    ("mouseDrag", "Mouse.Drag", "Unit"),
    ("mouseScroll", "Mouse.Scroll", "Unit"),
    ("resize", "Resize", "Unit"),
    ("signal", "Signal", "Unit"),
    ("waitTitle", "WaitTitle", "Unit"),
    ("expectTitle", "ExpectTitle", "Unit"),
    ("waitClipboard", "WaitClipboard", "Unit"),
    ("waitIdle", "WaitIdle", "Unit"),
    ("waitCommand", "WaitCommand", "Unit"),
    ("waitExit", "WaitExit", "Unit"),
    ("waitReady", "WaitReady", "Unit"),
    ("waitBell", "WaitBell", "Unit"),
    ("expectExitCode", "ExpectExitCode", "Unit"),
    ("expectOutput", "ExpectOutput", "Unit"),
    ("expectBellCount", "ExpectBellCount", "Unit"),
    ("snapshot", "Snapshot", "Snapshot"),
    ("screenshot", "Screenshot", "Screenshot"),
    ("startRecording", "StartRecording", "Unit"),
    ("stopRecording", "StopRecording", "Recording"),
];

const INPUTS: &[(&str, &str)] = &[
    ("Timeouts", "Timeouts"),
    ("AutomaticRecording", "AutomaticRecordingOptions"),
    ("TraceOptions", "TraceOptions"),
    ("FailureArtifactOptions", "FailureArtifactOptions"),
    ("OpenOptions", "OpenOptions"),
    ("RunOptions", "RunOptions"),
    ("TextStyle", "LocatorStyle"),
    ("LocatorNode", "LocatorNode"),
    ("LocatorExpression", "LocatorExpression"),
    ("Operation::WaitTitle", "TitleOptions"),
    ("Operation::WaitTitle", "ClipboardWaitOptions"),
    ("Operation::Snapshot", "SnapshotOptions"),
    ("Operation::Screenshot", "ScreenshotOptions"),
    ("Operation::StartRecording", "RecordingOptions"),
];

// These getters read current values without an intentional wait. Other operations
// retain Rust tasks so waits, input, and lifecycle work cannot exhaust Koffi's pool.
const SHORT_READS: &[&str] = &[
    "GetCommand",
    "GetOutput",
    "GetExitCode",
    "GetCwd",
    "GetTitle",
    "GetClipboard",
    "GetCursor",
    "GetSize",
    "GetViewport",
    "GetBellCount",
    "GetBellEvents",
];
const ENUMS: &[(&str, &str)] = &[
    ("Backend", "Backend"),
    ("Shell", "Shell"),
    ("RecordingFormat", "RecordingFormat"),
    ("SnapshotResult", "SnapshotResult"),
    ("UnderlineStyle", "UnderlineStyle"),
    ("LocatorNodeKind", "LocatorNodeKind"),
    ("LocatorDirection", "LocatorStageDirection"),
];

fn ts_type(ty: &str) -> Result<String> {
    if let Some(inner) = option(ty) {
        return Ok(format!("{} | null", ts_type(inner)?));
    }
    if let Some(inner) = ty.strip_prefix("Box<").and_then(|s| s.strip_suffix('>')) {
        return ts_type(inner);
    }
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
        return Ok(format!("Array<{}>", ts_type(inner)?));
    }
    Ok(match ty {
        "u8" | "u16" | "u32" | "u64" | "usize" | "i32" | "f64" | "MouseOptions" => "number".into(),
        "String"
        | "PathBuf"
        | "WhitespaceMode"
        | "TraceMode"
        | "FailureArtifactMode"
        | "ScreenshotResult" => "string".into(),
        "bool" => "boolean".into(),
        "()" => "void".into(),
        "(String,String)" => "[string, string]".into(),
        "BTreeMap<String,bool>" => "Record<string, boolean>".into(),
        "std::collections::BTreeMap<u8,String>" => "Record<string, string>".into(),
        "CellColor" => "Color".into(),
        "TextStyle" => "LocatorStyle".into(),
        "LocatorDirection" => "LocatorStageDirection".into(),
        "LocatorQuery" => "LocatorExpression".into(),
        t if OUTPUTS.contains(&t)
            || INPUTS.iter().any(|(_, target)| *target == t)
            || ENUMS.iter().any(|(_, target)| *target == t)
            || t == "MouseClickOptions" =>
        {
            t.into()
        }
        _ => bail!("no JavaScript mapping for {ty}"),
    })
}

fn parameter(mut f: Field) -> Field {
    f.name = match f.name.as_str() {
        "query" => "expression",
        "options" if f.ty == "MouseOptions" => "button",
        other => other,
    }
    .into();
    if f.ty == "bool" || matches!(f.name.as_str(), "w" | "h" | "clicks" | "amount" | "button") {
        f.ty = format!("Option<{}>", f.ty);
    }
    f
}

fn parameters(models: &Models, method: &str, variant: &str) -> Result<Vec<Field>> {
    let field = |name: &str, ty: &str| Field {
        name: name.into(),
        ty: ty.into(),
    };
    Ok(match method {
        "scroll" => {
            let mut fields = models.fields("ScrollTarget::Lines")?;
            fields[0].name = "lines".into();
            fields
        }
        "scrollToTop" => models.fields("ScrollTarget::Top")?,
        "scrollToBottom" => models.fields("ScrollTarget::Bottom")?,
        "mouseClick" => vec![field("options", "Option<MouseClickOptions>")],
        "waitTitle" | "expectTitle" => vec![
            field("text", "String"),
            field("options", "Option<TitleOptions>"),
        ],
        "waitClipboard" => vec![
            field("text", "Option<String>"),
            field("options", "Option<ClipboardWaitOptions>"),
        ],
        "snapshot" => vec![
            field("name", "String"),
            field("options", "Option<SnapshotOptions>"),
        ],
        "screenshot" => vec![field("options", "Option<ScreenshotOptions>")],
        "startRecording" => vec![field("options", "RecordingOptions")],
        _ => {
            let mut fields = if variant.starts_with("Key.") {
                models
                    .fields("Operation::Key")?
                    .into_iter()
                    .filter(|f| f.name != "action")
                    .collect()
            } else if let Some(variant) = variant.strip_prefix("Mouse.") {
                models.fields(&format!("MouseAction::{variant}"))?
            } else {
                models.fields(&format!("Operation::{variant}"))?
            };
            if method == "findLocator" {
                fields.push(field("require_one", "bool"));
            }
            if method == "type" {
                fields[0].name = "text".into();
            }
            fields.into_iter().map(parameter).collect()
        }
    })
}

fn declaration(fields: &[Field]) -> Result<String> {
    fields
        .iter()
        .map(|f| {
            let (ty, suffix) = if let Some(inner) = option(&f.ty) {
                (inner, "?")
            } else {
                (f.ty.as_str(), "")
            };
            Ok(format!(
                "{}{suffix}: {}{}",
                js_key(&f.name),
                ts_type(ty)?,
                if suffix.is_empty() {
                    ""
                } else {
                    " | undefined | null"
                }
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(|fields| fields.join(", "))
}

fn model(
    models: &Models,
    model: &str,
    target: &str,
    fields: Vec<Field>,
    output: bool,
    dts: &mut String,
    schema: &mut String,
) -> Result<()> {
    writeln!(dts, "export interface {target} {{")?;
    writeln!(schema, "{target}: {{")?;
    for f in fields {
        let key = if output && matches!(model, "OpenResult" | "State" | "Cell" | "BellEvent") {
            f.name.clone()
        } else {
            js_key(&f.name)
        };
        let optional = !output && option(&f.ty).is_some();
        let ty = if optional {
            option(&f.ty).unwrap()
        } else {
            &f.ty
        };
        let ts = if model == "PackedScreen" && f.name == "utf8" {
            "Uint8Array".into()
        } else if model == "Cell" && f.name == "underline_style" {
            "UnderlineStyle".into()
        } else {
            ts_type(ty)?
        };
        writeln!(
            dts,
            "  {}{key}{}: {ts}",
            if model == "PackedScreen" {
                "readonly "
            } else {
                ""
            },
            if optional { "?" } else { "" }
        )?;
        writeln!(schema, "{key:?}: [{:?}, {:?}],", f.name, f.ty)?;
    }
    dts.push_str("}\n\n");
    schema.push_str("},\n");
    let _ = models;
    Ok(())
}

fn coverage(models: &Models) -> Result<()> {
    let results: BTreeSet<_> = METHODS
        .iter()
        .map(|(_, _, result)| *result)
        .chain(["Open", "Modes", "Colors"])
        .collect();
    let owned_results: BTreeSet<_> = models.enums["OperationResult"]
        .iter()
        .map(|variant| variant.name.as_str())
        .collect();
    if results != owned_results {
        bail!("OperationResult JavaScript coverage mismatch");
    }
    let declared: BTreeSet<_> = METHODS
        .iter()
        .map(|(_, op, _)| op.split('.').next().unwrap())
        .chain([
            "Open",
            "Run",
            "Close",
            "FinishTrace",
            "ResolveLocator",
            "WaitClipboardMatch",
            "GetModes",
            "GetColors",
            "ExpectMode",
            "ExpectColors",
            "ExpectCursor",
        ])
        .collect();
    let owned: BTreeSet<_> = models.enums["Operation"]
        .iter()
        .map(|variant| variant.name.as_str())
        .collect();
    if declared != owned {
        bail!("Operation JavaScript coverage mismatch");
    }
    for (kind, model) in [
        ("Mouse", "MouseAction"),
        ("Key", "KeyAction"),
        ("Scroll", "ScrollTarget"),
    ] {
        let declared: BTreeSet<_> = METHODS
            .iter()
            .filter_map(|(_, op, _)| op.strip_prefix(&format!("{kind}.")))
            .collect();
        let owned: BTreeSet<_> = models.enums[model]
            .iter()
            .map(|variant| variant.name.as_str())
            .collect();
        if declared != owned {
            bail!("{model} JavaScript coverage mismatch");
        }
    }
    Ok(())
}

pub(super) fn generate(models: &Models, root: &Path, check: bool) -> Result<()> {
    coverage(models)?;
    let mut dts = format!("// {NOTICE}\nexport type Color = number | string;\n\n");
    let mut schema = format!("// {NOTICE}\nexport const models = {{\n");
    for &name in OUTPUTS {
        model(
            models,
            name,
            name,
            models.fields(name)?,
            true,
            &mut dts,
            &mut schema,
        )?;
    }
    for &(name, target) in INPUTS {
        let mut fields = input_fields(models, name, true)?;
        if target == "ClipboardWaitOptions" {
            fields.retain(|f| f.name != "not");
        }
        model(models, name, target, fields, false, &mut dts, &mut schema)?;
    }
    model(
        models,
        "MouseClickOptions",
        "MouseClickOptions",
        models
            .fields("MouseAction::Click")?
            .into_iter()
            .map(parameter)
            .collect(),
        false,
        &mut dts,
        &mut schema,
    )?;
    schema.push_str("};\nexport const enums = {\n");
    let mut exports = String::new();
    for &(name, target) in ENUMS {
        writeln!(dts, "export declare const enum {target} {{")?;
        writeln!(schema, "{target}: {{")?;
        for variant in &models.enums[name] {
            writeln!(dts, "  {} = {:?},", variant.name, variant.wire_name)?;
            writeln!(schema, "{}: {:?},", variant.name, variant.wire_name)?;
        }
        dts.push_str("}\n\n");
        schema.push_str("},\n");
        writeln!(exports, "export const {target} = enums.{target};")?;
    }
    schema.push_str("};\n");
    schema.push_str(&exports);
    schema.push_str("export const methods = {\n");
    dts.push_str("export declare class NativeSession {\n  constructor(name: string, recording?: AutomaticRecordingOptions | null, artifacts?: FailureArtifactOptions | null, trace?: TraceOptions | null)\n  name(): string\n  open(options?: OpenOptions | null): Promise<OpenResult>\n  run(options: RunOptions): Promise<OpenResult>\n  close(failed?: boolean | null): Promise<void>\n  panicProbe(): Promise<void>\n");
    for &(method, variant, result) in METHODS {
        let fields = parameters(models, method, variant)?;
        let result_fields = models.fields(&format!("OperationResult::{result}"))?;
        let ty = match result_fields.as_slice() {
            [] => "()",
            [field] => &field.ty,
            _ => bail!("unsupported result shape: {result}"),
        };
        writeln!(
            dts,
            "  {method}({}): Promise<{}>",
            declaration(&fields)?,
            ts_type(ty)?
        )?;
        write!(schema, "{method}: {{ result: {result:?}, ")?;
        if SHORT_READS.contains(&variant) {
            schema.push_str("short: true, ");
        }
        schema.push_str("args: [");
        for f in fields {
            write!(schema, "[{:?}, {:?}],", js_key(&f.name), f.ty)?;
        }
        schema.push_str("] },\n");
    }
    dts.push_str("}\n\nexport declare function sessions(): Promise<Array<string>>;\nexport declare function recording(name: string): Promise<string>;\nexport declare function closeAll(): Promise<void>;\nexport declare function closeAllSync(): void;\n");
    schema.push_str("};\n");
    emit(root, "bindings/js/native/index.d.ts", &dts, check)?;
    emit(root, "bindings/js/native/models.js", &schema, check)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_fields_drive_declarations_and_validation() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let mut models = Models::load(root).unwrap();
        models
            .parse("struct Cursor { generation_probe: bool }")
            .unwrap();
        let (mut dts, mut schema) = Default::default();
        model(
            &models,
            "Cursor",
            "Cursor",
            models.fields("Cursor").unwrap(),
            true,
            &mut dts,
            &mut schema,
        )
        .unwrap();
        assert!(dts.contains("generationProbe: boolean"));
        assert!(schema.contains("[\"generation_probe\", \"bool\"]"));
        models
            .parse("enum Operation { Resize { cols: String, rows: u16 } }")
            .unwrap();
        let fields = parameters(&models, "resize", "Resize").unwrap();
        assert_eq!(declaration(&fields).unwrap(), "cols: string, rows: number");
    }
    #[test]
    fn new_operations_require_an_export_decision() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        for name in [
            "Operation",
            "OperationResult",
            "MouseAction",
            "KeyAction",
            "ScrollTarget",
        ] {
            let mut models = Models::load(root).unwrap();
            models.enums.get_mut(name).unwrap().push(EnumVariant {
                name: "Future".into(),
                wire_name: "future".into(),
            });
            assert!(coverage(&models).is_err());
        }
    }

    #[test]
    fn owned_inputs_enums_and_result_types_drive_javascript() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let mut models = Models::load(root).unwrap();
        let source = fs::read_to_string(root.join("crates/tui-test/src/api.rs")).unwrap();
        models
            .parse(&source.replace("ExitCode(Option<i32>)", "ExitCode(Option<String>)"))
            .unwrap();
        models.parse("struct Timeouts { new_wait: Option<u64> } struct TextStyle { new_predicate: Option<bool> } enum RecordingFormat { Cast, Future }").unwrap();
        let output = super::super::tests::directory();
        generate(&models, output.path(), false).unwrap();
        let dts = fs::read_to_string(output.path().join("bindings/js/native/index.d.ts")).unwrap();
        let schema =
            fs::read_to_string(output.path().join("bindings/js/native/models.js")).unwrap();
        assert!(dts.contains("newWait?: number"));
        assert!(dts.contains("newPredicate?: boolean"));
        assert!(dts.contains("Future = \"future\""));
        assert!(dts.contains("getExitCode(): Promise<string | null>"));
        assert!(dts.contains("getViewport(): Promise<Viewport>"));
        assert!(dts.contains("scroll(lines: number): Promise<void>"));
        assert!(dts.contains("scrollToTop(): Promise<void>"));
        assert!(dts.contains("scrollToBottom(): Promise<void>"));
        assert!(schema.contains("[\"new_wait\", \"Option<u64>\"]"));
        assert!(schema.contains("[\"new_predicate\", \"Option<bool>\"]"));
        assert!(schema.contains("getSize: { result: \"Size\", short: true,"));
        assert!(schema.contains("getViewport: { result: \"Viewport\", short: true,"));
        assert!(schema.contains("scroll: { result: \"Unit\", args: [[\"lines\", \"i32\"]"));
        assert!(schema.contains("waitIdle: { result: \"Unit\", args:"));
        assert!(schema.contains("write: { result: \"Unit\", args:"));
    }
}
