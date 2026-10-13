//! JavaScript method names, defaults, and result selection are transport metadata.
//! Field types and operation construction come from the owned Rust enums.
use super::{assignment, ident, js_key, js_output, js_type, option, snake, Field, Models};
use crate::NOTICE;
use anyhow::{bail, Context, Result};
use quote::ToTokens;
use std::{collections::BTreeSet, fmt::Write, fs, path::Path};
use syn::{FnArg, ImplItem, Item, Pat};

// (JavaScript method in Rust spelling, owned variant, result selector, trace name).
// A dotted variant binds a nested MouseAction or a fixed KeyAction.
const METHODS: &[(&str, &str, &str, &str)] = &[
    ("restart", "Restart", "Open", "restart"),
    ("state", "State", "State", "state"),
    ("text", "Text", "Text", "text"),
    (
        "packed_screen",
        "PackedScreen",
        "PackedScreen",
        "packedScreen",
    ),
    ("cells", "Cells", "Cells", "cells"),
    (
        "find_locator",
        "FindLocator",
        "Matches",
        "locator.locations",
    ),
    ("wait_locator", "WaitLocator", "Unit", "locator.wait"),
    ("expect_locator", "WaitLocator", "Unit", "locator.expect"),
    ("click_locator", "ClickLocator", "Unit", "locator.click"),
    (
        "highlight_locator",
        "HighlightLocator",
        "Matches",
        "locator.highlight",
    ),
    ("get_command", "GetCommand", "Command", "getCommand"),
    ("get_output", "GetOutput", "Command", "getOutput"),
    ("get_exit_code", "GetExitCode", "ExitCode", "getExitCode"),
    ("get_cwd", "GetCwd", "Command", "getCwd"),
    ("get_title", "GetTitle", "Command", "getTitle"),
    ("get_clipboard", "GetClipboard", "Text", "getClipboard"),
    ("get_cursor", "GetCursor", "Cursor", "getCursor"),
    ("get_size", "GetSize", "Size", "getSize"),
    (
        "get_bell_count",
        "GetBellCount",
        "BellCount",
        "getBellCount",
    ),
    (
        "get_bell_events",
        "GetBellEvents",
        "BellEvents",
        "getBellEvents",
    ),
    ("write", "Write", "Unit", "write"),
    ("type_text", "Write", "Unit", "type"),
    ("submit", "Submit", "Unit", "submit"),
    ("press", "Key.Press", "Unit", "press"),
    ("key_down", "Key.Down", "Unit", "keyDown"),
    ("repeat", "Key.Repeat", "Unit", "repeat"),
    ("key_up", "Key.Up", "Unit", "keyUp"),
    ("mouse_click", "Mouse.Click", "Unit", "mouse.click"),
    ("mouse_move", "Mouse.Move", "Unit", "mouse.move"),
    ("mouse_down", "Mouse.Down", "Unit", "mouse.down"),
    ("mouse_up", "Mouse.Up", "Unit", "mouse.up"),
    ("mouse_drag", "Mouse.Drag", "Unit", "mouse.drag"),
    ("mouse_scroll", "Mouse.Scroll", "Unit", "mouse.scroll"),
    ("resize", "Resize", "Unit", "resize"),
    ("signal", "Signal", "Unit", "signal"),
    ("wait_title", "WaitTitle", "Unit", "waitTitle"),
    ("expect_title", "ExpectTitle", "Unit", "expectTitle"),
    ("wait_clipboard", "WaitClipboard", "Unit", "waitClipboard"),
    ("wait_idle", "WaitIdle", "Unit", "waitIdle"),
    ("wait_command", "WaitCommand", "Unit", "waitCommand"),
    ("wait_exit", "WaitExit", "Unit", "waitExit"),
    ("wait_ready", "WaitReady", "Unit", "waitReady"),
    ("wait_bell", "WaitBell", "Unit", "waitBell"),
    (
        "expect_exit_code",
        "ExpectExitCode",
        "Unit",
        "expectExitCode",
    ),
    ("expect_output", "ExpectOutput", "Unit", "expectOutput"),
    (
        "expect_bell_count",
        "ExpectBellCount",
        "Unit",
        "expectBellCount",
    ),
    ("snapshot", "Snapshot", "Snapshot", "expectSnapshot"),
    ("screenshot", "Screenshot", "Text", "screenshot"),
    (
        "start_recording",
        "StartRecording",
        "Unit",
        "startRecording",
    ),
    ("stop_recording", "StopRecording", "Text", "stopRecording"),
];

// These variants have lifecycle/selection semantics rather than direct methods.
const SPECIAL_OPERATIONS: &[&str] = &[
    "Open",
    "Run",
    "Close",
    "FinishTrace",
    "ResolveLocator",
    "WaitClipboardMatch",
];
// These operations have no native JavaScript method. Additions must be classified explicitly.
const UNEXPORTED_OPERATIONS: &[&str] = &[
    "GetModes",
    "GetColors",
    "ExpectMode",
    "ExpectColors",
    "ExpectCursor",
];
const LIFECYCLE: &[&str] = &["new", "name", "open", "run", "close"];
const GLOBALS: &[&str] = &["sessions", "close_all_sync", "recording"];

fn coverage(models: &Models) -> Result<()> {
    for (model, declared) in [
        (
            "Operation",
            METHODS
                .iter()
                .map(|m| m.1.split('.').next().unwrap())
                .chain(SPECIAL_OPERATIONS.iter().copied())
                .chain(UNEXPORTED_OPERATIONS.iter().copied())
                .collect::<BTreeSet<_>>(),
        ),
        (
            "MouseAction",
            METHODS
                .iter()
                .filter_map(|m| m.1.strip_prefix("Mouse."))
                .collect(),
        ),
        (
            "KeyAction",
            METHODS
                .iter()
                .filter_map(|m| m.1.strip_prefix("Key."))
                .collect(),
        ),
    ] {
        let owned: BTreeSet<_> = models
            .enums
            .get(model)
            .context("missing operation enum")?
            .iter()
            .map(String::as_str)
            .collect();
        if owned != declared {
            bail!(
                "{model} N-API coverage mismatch: {:?}",
                owned.symmetric_difference(&declared).collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

/// All exported attributes must reside in outputs that generation replaces.
pub(super) fn check_exports(root: &Path) -> Result<()> {
    fn scan(path: &Path, root: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                scan(&path, root)?;
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && !["napi/types.rs", "napi/bindings_generated.rs"]
                    .iter()
                    .any(|output| path == root.join(output))
            {
                let source = fs::read_to_string(&path)?;
                // Normalized tokens include attributes inside impls, modules, and macros.
                let tokens = syn::parse_file(&source)?
                    .to_token_stream()
                    .to_string()
                    .replace(' ', "");
                if tokens.contains("napi_derive")
                    || tokens.contains("#[napi")
                    || tokens.contains("#[::napi")
                    || (tokens.contains("#[cfg_attr(") && tokens.contains(",napi"))
                {
                    bail!("handwritten N-API export in {}", path.display());
                }
            }
        }
        Ok(())
    }
    let source = root.join("crates/tui-test-ffi/src");
    scan(&source, &source)
}

fn parameter(method: &str, field: &Field, prefix: &str) -> Result<(String, String, String)> {
    let name = match field.name.as_str() {
        "query" => "expression",
        "options" if field.ty == "MouseOptions" => "button",
        "data" if method == "type_text" => "text",
        other => other,
    }
    .to_owned();
    let ty = input_type(&field.ty)?;
    let default = match field.name.as_str() {
        "w" | "h" | "clicks" => Some("1.0"),
        "amount" => Some("3.0"),
        "options" if field.ty == "MouseOptions" => Some("0.0"),
        _ if field.ty == "bool" => Some("false"),
        _ => None,
    };
    let ty = if default.is_some() {
        format!("Option<{ty}>")
    } else {
        ty
    };
    let value = if let Some(default) = default {
        format!("{prefix}{name}.unwrap_or({default})")
    } else {
        format!("{prefix}{name}")
    };
    Ok((
        name.clone(),
        ty,
        input_value(&field.ty, &value, &js_key(&name))?,
    ))
}

fn input_type(ty: &str) -> Result<String> {
    if let Some(inner) = option(ty) {
        return Ok(format!("Option<{}>", input_type(inner)?));
    }
    Ok(match ty {
        "u8" | "u16" | "u32" | "u64" | "usize" | "i32" | "MouseOptions" => "f64".into(),
        "LocatorQuery" => "LocatorExpression".into(),
        _ => js_type(ty)?,
    })
}

fn input_value(ty: &str, value: &str, label: &str) -> Result<String> {
    if let Some(inner) = option(ty) {
        if input_type(inner)? == inner {
            return Ok(value.into());
        }
        let conversion = input_value(inner, "v", label)?;
        let conversion = conversion
            .strip_suffix('?')
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Ok({conversion})"));
        return Ok(format!(
            "{value}.map(|v| -> Result<_> {{ {conversion} }}).transpose()?"
        ));
    }
    Ok(match ty {
        "u16" => format!("u16_value({value}, {label:?})?"),
        "u8" => format!("u8_value({value}, {label:?})?"),
        "i32" => format!("i32_value({value}, {label:?})?"),
        "u64" => format!("integer({value}, {label:?}, u64::MAX)?"),
        "u32" | "usize" => format!("integer({value}, {label:?}, {ty}::MAX as u64)? as {ty}"),
        "LocatorQuery" => format!("query({value})?"),
        "MouseOptions" => format!("mouse_options({value})?"),
        _ => {
            input_type(ty)?;
            value.into()
        }
    })
}

pub(super) fn types(models: &Models, js: &mut String) -> Result<()> {
    js.push_str("#[napi] pub type Color = Either<u32,String>;\n#[derive(Default)] #[napi(object)] pub struct MouseClickOptions {\n");
    for field in models.fields("MouseAction::Click")? {
        let (name, ty, _) = parameter("mouse_click", &field, "")?;
        writeln!(js, "pub {}: {},", ident(&name), ty)?;
    }
    js.push_str("}\n");
    Ok(())
}

// Result unions preserve the existing adapter contract. The first variant supplies the type.
const RESULTS: &[(&str, &[&str])] = &[
    ("Unit", &["Unit"]),
    ("Open", &["Open"]),
    ("State", &["State"]),
    ("Text", &["Text", "Clipboard", "Recording", "Screenshot"]),
    ("PackedScreen", &["PackedScreen"]),
    ("Cells", &["Cells"]),
    ("Matches", &["Matches"]),
    ("Command", &["Command", "Output", "Cwd", "Title"]),
    ("ExitCode", &["ExitCode"]),
    ("Cursor", &["Cursor"]),
    ("Size", &["Size"]),
    ("BellCount", &["BellCount"]),
    ("BellEvents", &["BellEvents"]),
    ("Snapshot", &["Snapshot"]),
];

fn result_type(models: &Models, variant: &str) -> Result<String> {
    let fields = models.fields(&format!("OperationResult::{variant}"))?;
    match fields.as_slice() {
        [] => Ok("()".into()),
        [field] => js_type(
            field
                .ty
                .strip_prefix("Box<")
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(&field.ty),
        )
        .or_else(|error| {
            if field.ty == "SnapshotResult" {
                Ok(field.ty.clone())
            } else {
                Err(error)
            }
        }),
        _ => bail!("unsupported result shape: {variant}"),
    }
}

fn result_name(variant: &str) -> String {
    match variant {
        "Unit" => "unit".into(),
        "Text" => "string_result".into(),
        "Command" => "optional_string_result".into(),
        "Matches" => "matches".into(),
        _ => format!("{}_result", snake(variant)),
    }
}

pub(super) fn results(models: &Models, out: &mut String) -> Result<()> {
    let covered: BTreeSet<_> = RESULTS
        .iter()
        .flat_map(|r| r.1.iter().copied())
        .chain(["Modes", "Colors"])
        .collect();
    let owned: BTreeSet<_> = models.enums["OperationResult"]
        .iter()
        .map(String::as_str)
        .collect();
    if covered != owned {
        bail!("OperationResult N-API coverage mismatch");
    }
    for (selector, variants) in RESULTS {
        writeln!(
            out,
            "pub fn {}(value: OperationResult) -> Result<{}> {{ match value {{",
            result_name(selector),
            result_type(models, selector)?
        )?;
        for variant in *variants {
            let fields = models.fields(&format!("OperationResult::{variant}"))?;
            let (pattern, conversion) = match fields.as_slice() {
                [] => (format!("OperationResult::{variant}"), "Ok(())".into()),
                [field] => {
                    let (ty, value) = if let Some(inner) = field
                        .ty
                        .strip_prefix("Box<")
                        .and_then(|t| t.strip_suffix('>'))
                    {
                        (inner, "*value")
                    } else {
                        (field.ty.as_str(), "value")
                    };
                    let conversion = match ty {
                        "Vec<Cell>" => "value.into_iter().map(cell).collect()".into(),
                        "SnapshotResult" => "Ok(value.into())".into(),
                        "ScreenshotResult" => {
                            let mut arms = String::new();
                            for variant in &models.enums["ScreenshotResult"] {
                                let fields =
                                    models.fields(&format!("ScreenshotResult::{variant}"))?;
                                if fields.len() != 1 || fields[0].ty != "String" {
                                    bail!("unsupported screenshot result");
                                }
                                write!(
                                    arms,
                                    "api::ScreenshotResult::{variant}(value) => Ok(value),"
                                )?;
                            }
                            format!("match value {{ {arms} }}")
                        }
                        _ => format!("Ok({})", js_output(ty, value)?),
                    };
                    (format!("OperationResult::{variant}(value)"), conversion)
                }
                _ => bail!("unsupported result shape: {variant}"),
            };
            writeln!(out, "{pattern} => {conversion},")?;
        }
        out.push_str(
            "_ => Err(internal(\"native operation returned an unexpected result\")), } }\n",
        );
    }
    Ok(())
}

fn operation_method(
    models: &Models,
    method: &str,
    source: &str,
    result: &str,
    trace: &str,
) -> Result<String> {
    let (operation, nested) = source
        .split_once('.')
        .map_or((source, None), |(a, b)| (a, Some(b)));
    let model = if operation == "Mouse" {
        format!("MouseAction::{}", nested.unwrap())
    } else {
        format!("Operation::{operation}")
    };
    let mut params = Vec::new();
    let mut assignments = String::new();
    let mut setup = String::new();
    let mut expression = String::new();
    let options = match operation {
        "WaitTitle" | "ExpectTitle" => Some(("TitleOptions", Some("text"))),
        "Snapshot" => Some(("SnapshotOptions", Some("name"))),
        "Screenshot" => Some(("ScreenshotOptions", None)),
        "StartRecording" => Some(("RecordingOptions", None)),
        _ => None,
    };
    if let Some((dto, separate)) = options {
        if let Some(name) = separate {
            let field = models
                .fields(&model)?
                .into_iter()
                .find(|f| f.name == name)
                .context("separate option argument")?;
            params.push(format!("{name}: {}", input_type(&field.ty)?));
        }
        let optional = operation != "StartRecording";
        params.push(format!(
            "options: {}",
            if optional {
                format!("Option<{dto}>")
            } else {
                dto.into()
            }
        ));
        if optional {
            setup.push_str("let options = options.unwrap_or_default();");
        }
        expression = format!(
            "{}_operation(options{})",
            snake(operation),
            separate.map(|s| format!(", {s}")).unwrap_or_default()
        );
    } else if operation == "WaitClipboard" {
        // This JS overload chooses between two operations and validates a regex.
        let text = models
            .fields("ClipboardPattern::Text")?
            .into_iter()
            .next()
            .context("clipboard text")?;
        params.push(format!("text: Option<{}>", input_type(&text.ty)?));
        params.push("options: Option<ClipboardWaitOptions>".into());
        setup.push_str("let options = options.unwrap_or_default();");
        expression = "wait_clipboard_operation(text, options)".into();
    } else {
        let grouped = source == "Mouse.Click";
        if grouped {
            params.push("options: Option<MouseClickOptions>".into());
            setup.push_str("let options = options.unwrap_or_default();");
        }
        for field in models.fields(&model)? {
            let value = if operation == "Key" && field.name == "action" {
                format!("KeyAction::{}", nested.context("key action")?)
            } else {
                let (name, ty, value) =
                    parameter(method, &field, if grouped { "options." } else { "" })?;
                if !grouped {
                    params.push(format!("{name}: {ty}"));
                }
                value
            };
            assignment(&mut assignments, ident(&field.name), &value)?;
        }
        let value = if assignments.is_empty() {
            model.clone()
        } else {
            format!("{model} {{ {assignments} }}")
        };
        let value = if operation == "Mouse" {
            format!("Operation::Mouse {{ action: {value} }}")
        } else {
            value
        };
        write!(expression, "Ok({value})")?;
    }
    let trace = if method == "find_locator" {
        params.push("require_one: Option<bool>".into());
        setup.push_str("let require_one = require_one.unwrap_or(false);");
        expression = format!("Ok(if require_one {{ Operation::ResolveLocator {{ {assignments} }} }} else {{ Operation::FindLocator {{ {assignments} }} }})");
        "if require_one { \"locator.location\" } else { \"locator.locations\" }".into()
    } else {
        format!("{trace:?}")
    };
    Ok(format!(
        "#[napi{}] pub async fn {method}(&self{}) -> Result<{}> {{ {setup} self.inner.call({trace}, move || {{ {expression} }}, {}).await }}\n",
        if method == "type_text" { "(js_name = \"type\")" } else { "" },
        params.iter().map(|p| format!(", {p}")).collect::<String>(),
        result_type(models, result)?, result_name(result),
    ))
}

/// Lifecycle signatures come from the real runtime functions, not a parallel declaration.
fn forward(sig: &syn::Signature, receiver: bool) -> Result<String> {
    let name = sig.ident.to_string();
    let args = sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(arg) => Some(match arg.pat.as_ref() {
                Pat::Ident(pat) => Ok(pat.ident.to_string()),
                _ => Err(anyhow::anyhow!("unsupported runtime argument")),
            }),
            FnArg::Receiver(_) => None,
        })
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let target = if receiver {
        if name == "new" {
            "SessionRuntime::"
        } else {
            "self.inner."
        }
    } else {
        "super::"
    };
    Ok(format!(
        "#[napi{}] pub {} {{ {target}{name}({args}){}{} }}\n",
        if name == "new" { "(constructor)" } else { "" },
        sig.to_token_stream(),
        if sig.asyncness.is_some() {
            ".await"
        } else {
            ""
        },
        if name == "new" {
            ".map(|inner| Self { inner })"
        } else {
            ""
        },
    ))
}

pub(super) fn bindings(models: &Models, root: &Path) -> Result<String> {
    coverage(models)?;
    let runtime = syn::parse_file(&fs::read_to_string(
        root.join("crates/tui-test-ffi/src/napi/mod.rs"),
    )?)?;
    let mut lifecycle = std::collections::BTreeMap::new();
    let mut globals = std::collections::BTreeMap::new();
    for item in runtime.items {
        match item {
            Item::Impl(item) if item.self_ty.to_token_stream().to_string() == "SessionRuntime" => {
                for item in item.items {
                    if let ImplItem::Fn(item) = item {
                        if LIFECYCLE.contains(&item.sig.ident.to_string().as_str()) {
                            lifecycle.insert(item.sig.ident.to_string(), item.sig);
                        }
                    }
                }
            }
            Item::Fn(item) if GLOBALS.contains(&item.sig.ident.to_string().as_str()) => {
                globals.insert(item.sig.ident.to_string(), item.sig);
            }
            _ => {}
        }
    }
    let mut out = format!("// {NOTICE}\nuse super::*;\nuse napi_derive::napi;\nuse tui_test::api::{{KeyAction, MouseAction}};\n#[napi] pub struct NativeSession {{ inner: SessionRuntime }}\n#[napi] impl NativeSession {{\n");
    for name in LIFECYCLE.iter().filter(|name| **name != "close") {
        out.push_str(&forward(
            lifecycle
                .get(*name)
                .with_context(|| format!("missing lifecycle method {name}"))?,
            true,
        )?);
    }
    for &(method, source, result, trace) in METHODS {
        out.push_str(&operation_method(models, method, source, result, trace)?);
        // Keep the published declaration order stable.
        if method == "restart" {
            out.push_str(&forward(
                lifecycle
                    .get("close")
                    .context("missing lifecycle method close")?,
                true,
            )?);
        }
        if method == "highlight_locator" {
            out.push_str("#[napi] pub async fn panic_probe(&self) -> Result<()> { blocking(|| panic!(\"intentional native panic probe\")).await }\n");
        }
    }
    out.push_str("}\n");
    for name in GLOBALS {
        out.push_str(&forward(
            globals
                .get(*name)
                .with_context(|| format!("missing global {name}"))?,
            false,
        )?);
    }
    out.push_str("#[napi] pub async fn close_all() -> Result<()> { blocking(super::close_all_sync).await }\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    fn operation_fields_drive_parameters_validation_and_dispatch() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("enum Operation { Resize { cols: u16, rows: String, new_limit: Option<u64> } }")
            .unwrap();
        let output = operation_method(&models, "resize", "Resize", "Unit", "resize").unwrap();
        assert!(output.contains("cols: f64, rows: String, new_limit: Option<f64>"));
        assert!(output.contains("cols: u16_value(cols, \"cols\")?"));
        assert!(output.contains("rows,"));
        assert!(output.contains("new_limit: new_limit.map"));
        assert!(output.contains("integer(v, \"newLimit\", u64::MAX)"));
        assert!(output.contains("Operation::Resize"));
        models
            .parse("enum Operation { Resize { unknown: Unsupported } }")
            .unwrap();
        assert!(operation_method(&models, "resize", "Resize", "Unit", "resize").is_err());
    }

    #[test]
    fn mouse_fields_drive_both_options_and_method_conversion() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("enum MouseAction { Click { value: Option<u16> } }")
            .unwrap();
        let mut declarations = String::new();
        types(&models, &mut declarations).unwrap();
        let method =
            operation_method(&models, "mouse_click", "Mouse.Click", "Unit", "mouse.click").unwrap();
        assert!(declarations.contains("pub value: Option<f64>"));
        assert!(method.contains("value: options.value.map"));
        assert!(method.contains("u16_value(v, \"value\")"));
    }

    #[test]
    fn result_payload_drives_method_signature_and_selection() {
        let mut models = Models::load(root()).unwrap();
        let source = fs::read_to_string(root().join("crates/tui-test/src/api.rs")).unwrap();
        models
            .parse(&source.replace("ExitCode(Option<i32>)", "ExitCode(Option<String>)"))
            .unwrap();
        let method = operation_method(
            &models,
            "get_exit_code",
            "GetExitCode",
            "ExitCode",
            "getExitCode",
        )
        .unwrap();
        assert!(method.contains("Result<Option<String>>"));
        let mut conversions = String::new();
        results(&models, &mut conversions).unwrap();
        assert!(conversions
            .contains("fn exit_code_result(value: OperationResult) -> Result<Option<String>>"));
        assert!(conversions.contains("OperationResult::ExitCode(value) => Ok(value)"));
    }

    #[test]
    fn new_variants_require_an_explicit_export_decision() {
        for model in ["Operation", "MouseAction", "KeyAction", "OperationResult"] {
            let mut models = Models::load(root()).unwrap();
            models
                .enums
                .get_mut(model)
                .unwrap()
                .push("NewVariant".into());
            let result = if model == "OperationResult" {
                results(&models, &mut String::new())
            } else {
                coverage(&models)
            };
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("coverage mismatch"),
                "{model}"
            );
        }
    }

    #[test]
    fn lifecycle_wrappers_follow_runtime_signatures() {
        let item: syn::ItemFn = syn::parse_str("async fn open(&self, options: Option<OpenOptions>, probe: bool) -> Result<OpenResult> {}").unwrap();
        let wrapper = forward(&item.sig, true).unwrap();
        assert!(wrapper.contains("probe : bool"));
        assert!(wrapper.contains("self.inner.open(options,probe).await"));
        let item: syn::ItemFn = syn::parse_str("fn new(name: String) -> Result<Self> {}").unwrap();
        let wrapper = forward(&item.sig, true).unwrap();
        assert!(wrapper.contains("#[napi(constructor)]"));
        assert!(wrapper.contains("SessionRuntime::new(name).map(|inner| Self { inner })"));
    }

    #[test]
    fn every_native_export_is_generated_and_matches_the_published_surface() {
        check_exports(root()).unwrap();
        let models = Models::load(root()).unwrap();
        let output = bindings(&models, root()).unwrap();
        let mut methods = BTreeSet::new();
        let parsed = syn::parse_file(&output).unwrap();
        for item in parsed.items {
            if let Item::Impl(item) = item {
                for item in item.items {
                    if let ImplItem::Fn(item) = item {
                        assert!(item.attrs.iter().any(|a| a.path().is_ident("napi")));
                        let name = item.sig.ident.to_string();
                        methods.insert(match name.as_str() {
                            "new" => "constructor".into(),
                            "type_text" => "type".into(),
                            _ => js_key(&name),
                        });
                    }
                }
            }
        }
        let declarations =
            fs::read_to_string(root().join("bindings/js/native/index.d.ts")).unwrap();
        let published = declarations
            .split("export declare class NativeSession {")
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap()
            .lines()
            .filter_map(|line| line.trim().split_once('(').map(|(name, _)| name.to_owned()))
            .collect::<BTreeSet<_>>();
        assert_eq!(methods, published);
        for name in GLOBALS {
            assert!(output.contains(&format!("fn {name}")));
        }
        assert!(output.contains("fn close_all()"));
    }

    #[test]
    fn manual_exports_cannot_hide_in_helpers_or_nested_modules() {
        let directory = super::super::tests::directory();
        let source = directory.path().join("crates/tui-test-ffi/src");
        fs::create_dir_all(source.join("napi/nested")).unwrap();
        for (path, text) in [
            ("helper.rs", "#[napi] pub fn extra() {}"),
            (
                "napi/nested/types.rs",
                "mod nested { #[napi(object)] pub struct Extra {} }",
            ),
            (
                "napi/nested/types.rs",
                "macro_rules! exports { () => { # [ napi ] pub fn extra() {} } }",
            ),
            (
                "napi/nested/types.rs",
                "#[cfg_attr(feature = \"napi\", napi)] pub fn extra() {}",
            ),
            (
                "napi/nested/types.rs",
                "use napi_derive::napi as export; #[export] pub fn extra() {}",
            ),
        ] {
            let path = source.join(path);
            fs::write(&path, text).unwrap();
            let error = check_exports(directory.path()).unwrap_err().to_string();
            assert!(error.contains("handwritten N-API export"), "{error}");
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn trace_names_defaults_and_selectors_remain_explicit() {
        let models = Models::load(root()).unwrap();
        let output = bindings(&models, root()).unwrap();
        for (_, _, _, trace) in METHODS {
            assert!(output.contains(&format!("{trace:?}")), "{trace}");
        }
        for expression in [
            "if require_one { \"locator.location\" } else { \"locator.locations\" }",
            "Operation::ResolveLocator",
            "Operation::FindLocator",
            "full.unwrap_or(false)",
            "w.unwrap_or(1.0)",
            "h.unwrap_or(1.0)",
            "options.clicks.unwrap_or(1.0)",
            "button.unwrap_or(0.0)",
            "amount.unwrap_or(3.0)",
            "blocking(|| panic!(\"intentional native panic probe\"))",
        ] {
            assert!(output.contains(expression), "{expression}");
        }
    }
}
