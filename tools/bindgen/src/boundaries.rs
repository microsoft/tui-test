//! Fields and enums come from owned Rust models. Overrides describe transport contracts only.
use crate::{emit, format_source, NOTICE};
use anyhow::{bail, Context, Result};
use quote::ToTokens;
use std::{collections::BTreeMap, fmt::Write, fs, path::Path};
use syn::{Fields, Item};

#[path = "javascript.rs"]
mod javascript;

#[derive(Clone)]
struct Field {
    name: String,
    ty: String,
}

struct Models {
    fields: BTreeMap<String, Vec<Field>>,
    enums: BTreeMap<String, Vec<String>>,
}

impl Models {
    fn load(root: &Path) -> Result<Self> {
        let mut models = Self {
            fields: BTreeMap::new(),
            enums: BTreeMap::new(),
        };
        for module in [
            "api",
            "locator_query",
            "diagnostics",
            "profile",
            "shell/mod",
            "terminal/backend",
            "terminal/cell",
        ] {
            models.parse(&fs::read_to_string(
                root.join(format!("crates/tui-test/src/{module}.rs")),
            )?)?;
        }
        Ok(models)
    }

    fn parse(&mut self, source: &str) -> Result<()> {
        fn fields(values: &Fields) -> Vec<Field> {
            values
                .iter()
                .enumerate()
                .map(|(i, f)| Field {
                    name: f
                        .ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| i.to_string()),
                    ty: f.ty.to_token_stream().to_string().replace(' ', ""),
                })
                .collect()
        }
        for item in syn::parse_file(source)?.items {
            match item {
                Item::Struct(item) if matches!(item.fields, Fields::Named(_)) => {
                    self.fields
                        .insert(item.ident.to_string(), fields(&item.fields));
                }
                Item::Enum(item) => {
                    let name = item.ident.to_string();
                    self.enums.insert(
                        name.clone(),
                        item.variants.iter().map(|v| v.ident.to_string()).collect(),
                    );
                    for variant in item.variants {
                        self.fields.insert(
                            format!("{name}::{}", variant.ident),
                            fields(&variant.fields),
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn fields(&self, name: &str) -> Result<Vec<Field>> {
        self.fields
            .get(name)
            .cloned()
            .with_context(|| format!("missing model {name}"))
    }
}

const OUTPUTS: &[&str] = &[
    "OpenResult",
    "Cursor",
    "Size",
    "EffectiveTimeouts",
    "BellEvent",
    "State",
    "TerminalColors",
    "Cell",
    "TextPosition",
    "TextSpan",
    "TextMatch",
    "PackedScreen",
];

fn c_name(name: &str) -> String {
    format!(
        "Tui{}",
        match name {
            "EffectiveTimeouts" => "Timeouts",
            "TextPosition" => "Position",
            "TextSpan" => "Span",
            "TextMatch" => "Match",
            other => other,
        }
    )
}
fn ident(name: &str) -> &str {
    if name == "char" {
        "r#char"
    } else {
        name
    }
}
fn snake(name: &str) -> String {
    if name == "OpenResult" {
        return "open_value".into();
    }
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i != 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}
fn js_key(name: &str) -> String {
    let mut words = name.split('_');
    let mut out = words.next().unwrap_or_default().to_owned();
    for word in words {
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}
fn option(ty: &str) -> Option<&str> {
    ty.strip_prefix("Option<").and_then(|s| s.strip_suffix('>'))
}
fn c_type(ty: &str) -> Result<String> {
    Ok(match ty {
        "String" | "Option<String>" | "PathBuf" | "Option<PathBuf>" => "TuiString".into(),
        "Option<u64>" | "Option<u32>" | "Option<u8>" => "TuiOptionalU64".into(),
        "Option<i32>" => "TuiOptionalI32".into(),
        "Option<bool>" => "TuiOptionalBool".into(),
        "Option<f64>" => "TuiOptionalF64".into(),
        "CellColor" => "TuiColor".into(),
        "bool" | "u16" | "u32" | "u64" | "u8" | "f64" | "i32" => ty.into(),
        t if OUTPUTS.contains(&t) || t == "Timeouts" => c_name(t),
        _ => bail!("no C mapping for {ty}"),
    })
}
fn c_output(ty: &str, value: &str) -> Result<String> {
    Ok(match ty {
        "String" => format!("s.string({value})"),
        "Option<String>" => format!("s.optional({value})"),
        "Option<i32>" => format!("integer({value})"),
        "Option<u32>" => format!("TuiOptionalU64 {{ present: {value}.is_some(), value: u64::from({value}.unwrap_or_default()) }}"),
        "CellColor" => format!("color({value})"),
        t if OUTPUTS.contains(&t) => format!("{}(s, {value})", snake(t)),
        _ => { c_type(ty)?; value.into() }
    })
}

fn output_models(models: &Models, c: &mut String, cc: &mut String) -> Result<()> {
    for &model in OUTPUTS {
        if model == "PackedScreen" {
            continue;
        }
        let c_enabled = model != "EffectiveTimeouts";
        if c_enabled {
            writeln!(
                c,
                "#[repr(C)] #[derive(Clone,Copy,Default)] pub struct {} {{",
                c_name(model)
            )?;
        }
        writeln!(
            cc,
            "#[allow(unused_variables)] fn {}(s: &mut Storage, v: tui_test::api::{model}) -> {} {{",
            snake(model),
            c_name(model)
        )?;
        let mut init = String::new();
        for f in models.fields(model)? {
            let field = ident(&f.name);
            let cfield = if f.name == "char" { "character" } else { field };
            let value = format!("v.{field}");
            let array = match f.ty.as_str() {
                "Vec<TextSpan>" => Some(("TuiSpan", "spans")),
                "BTreeMap<String,bool>" => Some(("TuiMode", "modes")),
                "std::collections::BTreeMap<u8,String>" => Some(("TuiPair", "palette")),
                _ => None,
            };
            if c_enabled {
                if let Some((ty, _)) = array {
                    writeln!(c, "pub {cfield}: *const {ty}, pub {cfield}_len: usize,")?;
                } else {
                    writeln!(c, "pub {cfield}: {},", c_type(&f.ty)?)?;
                }
            }
            if let Some((_, helper)) = array {
                writeln!(cc, "let {cfield} = s.{helper}({value});")?;
                writeln!(init, "{cfield}: {cfield}.0, {cfield}_len: {cfield}.1,")?;
            } else {
                let expr = if model == "EffectiveTimeouts" {
                    format!("unsigned({value})")
                } else {
                    c_output(&f.ty, &value)?
                };
                writeln!(init, "{cfield}: {expr},")?;
            }
        }
        if c_enabled {
            c.push_str("}\n");
        }
        writeln!(cc, "{} {{ {init} }} }}", c_name(model))?;
    }
    Ok(())
}

/// Flattened options preserve the C v3 field order and the public JavaScript API.
fn input_fields(models: &Models, model: &str, js: bool) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    for mut f in models.fields(model)? {
        match (model, f.name.as_str()) {
            ("OpenOptions" | "RunOptions", "profile" | "style") => continue,
            ("OpenOptions" | "RunOptions", "recording") if js => continue,
            ("OpenOptions" | "RunOptions", "recording") => {
                for mut f in models.fields("AutomaticRecording")? {
                    f.name = format!("recording_{}", f.name);
                    f.ty = "TuiString".into();
                    fields.push(f);
                }
                continue;
            }
            ("OpenOptions" | "RunOptions", "timeouts") => {
                for mut p in models.fields("Profile")? {
                    p.name = if js {
                        format!("profile_{}", p.name)
                    } else {
                        p.name
                    };
                    p.ty = match p.ty.as_str() {
                        "usize" => {
                            if js {
                                "Option<u64>"
                            } else {
                                "TuiOptionalU64"
                            }
                        }
                        "Colors" => {
                            if js {
                                "Option<Vec<(String,String)>>"
                            } else {
                                "*const TuiPair"
                            }
                        }
                        other => bail!("no profile mapping for {other}"),
                    }
                    .into();
                    fields.push(p);
                    if !js && fields.last().unwrap().name == "colors" {
                        fields.push(Field {
                            name: "colors_len".into(),
                            ty: "usize".into(),
                        });
                    }
                }
            }
            ("AutomaticRecording", "mode")
            | ("Operation::Snapshot", "name")
            | ("Operation::WaitTitle", "text") => continue,
            ("Operation::Screenshot" | "Operation::StartRecording", "background") => {
                fields.push(Field {
                    name: f.name,
                    ty: if js { "Option<String>" } else { "TuiString" }.into(),
                });
                fields.push(Field {
                    name: "transparent".into(),
                    ty: if js { "Option<bool>" } else { "bool" }.into(),
                });
                continue;
            }
            _ => {}
        }
        f.ty = if js {
            match (model, f.name.as_str()) {
                ("OpenOptions" | "RunOptions", "backend" | "shell") => format!(
                    "Option<{}>",
                    if f.name == "backend" {
                        "Backend"
                    } else {
                        "Shell"
                    }
                ),
                ("OpenOptions" | "RunOptions", _)
                    if option(&f.ty).is_none() && f.name != "program" =>
                {
                    format!("Option<{}>", f.ty)
                }
                ("TraceOptions", _) | ("FailureArtifactOptions", "mode" | "include_recording") => {
                    format!("Option<{}>", f.ty)
                }
                ("LocatorNode", "occurrence") => "Option<String>".into(),
                ("Operation::WaitTitle" | "Operation::Snapshot" | "Operation::Screenshot", _)
                    if f.ty == "bool" =>
                {
                    "Option<bool>".into()
                }
                _ => f.ty,
            }
        } else {
            match (model, f.name.as_str()) {
                ("OpenOptions", "backend" | "shell") | ("Operation::StartRecording", "format") => {
                    "TuiString".into()
                }
                ("OpenOptions", "cols" | "rows") => "TuiOptionalU64".into(),
                ("OpenOptions", "env") => "*const TuiPair".into(),
                ("MouseOptions", "button") => "u32".into(),
                _ => c_type(&f.ty)?,
            }
        };
        let env = !js && f.name == "env";
        let nth = js && model == "LocatorNode" && f.name == "occurrence";
        fields.push(f);
        if env {
            fields.push(Field {
                name: "env_len".into(),
                ty: "usize".into(),
            });
        }
        if nth {
            fields.push(Field {
                name: "nth".into(),
                ty: "Option<usize>".into(),
            });
        }
    }
    if js && matches!(model, "OpenOptions" | "RunOptions") {
        for mut f in models.fields("DiagnosticRetentionOptions")? {
            f.ty = format!("Option<{}>", f.ty);
            fields.push(f);
        }
    }
    if !js && model == "Operation::WaitTitle" {
        fields.sort_by_key(|f| match f.name.as_str() {
            "timeout_ms" => 0,
            "regex" => 1,
            _ => 2,
        });
    }
    Ok(fields)
}

fn input_models(models: &Models, c: &mut String, ci: &mut String) -> Result<()> {
    for (model, target) in [
        ("Timeouts", "Timeouts"),
        ("OpenOptions", "OpenOptions"),
        ("MouseOptions", "MouseOptions"),
        ("Operation::WaitTitle", "WaitOptions"),
        ("Operation::StartRecording", "RecordingOptions"),
        ("Operation::Screenshot", "ScreenshotOptions"),
    ] {
        if model == "MouseOptions" {
            c.push_str("/// button: 0 left, 1 middle, 2 right.\n");
        }
        writeln!(
            c,
            "#[repr(C)] #[derive(Clone,Copy,Default)] pub struct Tui{target} {{"
        )?;
        for f in input_fields(models, model, false)? {
            let field = if f.name == "not" {
                "not_"
            } else {
                ident(&f.name)
            };
            writeln!(c, "pub {field}: {},", f.ty)?;
        }
        c.push_str("}\n");
    }
    ci.push_str("fn timeouts(value: TuiTimeouts) -> Timeouts { Timeouts {\n");
    for f in models.fields("Timeouts")? {
        writeln!(ci, "{}: value.{}.option(),", f.name, f.name)?;
    }
    ci.push_str("} }\n");
    Ok(())
}

fn assignment(out: &mut String, name: &str, expr: &str) -> Result<()> {
    if name == expr {
        writeln!(out, "{name},")?;
    } else {
        writeln!(out, "{name}: {expr},")?;
    }
    Ok(())
}

fn session_input(models: &Models) -> Result<String> {
    let mut out = format!("// {NOTICE}\n#[derive(Clone, Default, serde::Deserialize)] #[serde(deny_unknown_fields)] pub(crate) struct SessionInput {{ pub name: String,\n");
    for (model, prefix) in [
        ("FailureArtifactOptions", "artifact_"),
        ("TraceOptions", "trace_"),
        ("DiagnosticRetentionOptions", ""),
    ] {
        for f in models.fields(model)? {
            let ty = match f.ty.as_str() {
                "bool" => {
                    out.push_str("#[serde(default)]\n");
                    "bool"
                }
                "u16" => "Option<u16>",
                "PathBuf" | "FailureArtifactMode" | "TraceMode" => "Option<String>",
                _ => bail!("no session input mapping for {}", f.ty),
            };
            writeln!(out, "pub {prefix}{}: {ty},", f.name)?;
        }
    }
    out.push_str(
        "}\nimpl SessionInput { pub fn validate(self) -> Result<(String, ExecutionContext)> {\n",
    );
    for (model, prefix) in [
        ("FailureArtifactOptions", "artifact"),
        ("TraceOptions", "trace"),
    ] {
        writeln!(out, "let {prefix} = match self.{prefix}_mode {{ None => None, Some(mode) => {{ let mode = {prefix}_mode(&mode)?; let options = {model} {{")?;
        for f in models.fields(model)? {
            let expr = match f.ty.as_str() {
                "FailureArtifactMode" | "TraceMode" => "mode".into(),
                "PathBuf" if model == "FailureArtifactOptions" => {
                    format!("self.{prefix}_{}.unwrap_or_default().into()", f.name)
                }
                "PathBuf" => format!(
                    "self.{prefix}_{}.map(Into::into).unwrap_or_else(|| {model}::default().{})",
                    f.name, f.name
                ),
                _ => format!("self.{prefix}_{}", f.name),
            };
            assignment(&mut out, &f.name, &expr)?;
        }
        out.push_str("}; options.validate().map_err(TuiTestError::usage)?; Some(options) } };\n");
    }
    out.push_str("let defaults = DiagnosticRetentionOptions::default();\nlet retention = DiagnosticRetentionOptions {\n");
    for f in models.fields("DiagnosticRetentionOptions")? {
        writeln!(
            out,
            "{}: self.{}.unwrap_or(defaults.{}),",
            f.name, f.name, f.name
        )?;
    }
    out.push_str("}; retention.validate().map_err(TuiTestError::usage)?;\nOk((self.name, ExecutionContext { artifact, trace, retention, ..ExecutionContext::default() })) } }\n");
    Ok(out)
}

fn input_conversions(models: &Models, ci: &mut String) -> Result<()> {
    ci.push_str("unsafe fn open_input(value: TuiOpenOptions, backend: tui_test::terminal::backend::Backend, shell: Option<Shell>, profile: Profile, mode: AutomaticRecordingMode) -> Result<OpenOptions> { Ok(OpenOptions {\n");
    for f in models.fields("OpenOptions")? {
        let name = &f.name;
        let value = match name.as_str() {
            "backend" | "shell" | "profile" => name.clone(),
            "style" => "Default::default()".into(),
            "cols" => "u16_option(value.cols, \"cols\")?.unwrap_or(80)".into(),
            "rows" => "u16_option(value.rows, \"rows\")?.unwrap_or(30)".into(),
            "env" => "unsafe { pairs(value.env, value.env_len)? }".into(),
            "timeouts" => "timeouts(value.timeouts)".into(),
            "recording" => "AutomaticRecording { mode, directory: unsafe { value.recording_directory.optional()? }.map(Into::into) }".into(),
            _ if f.ty == "Option<String>" => format!("unsafe {{ value.{name}.optional()? }}"),
            _ if option(&f.ty).is_some() => format!("value.{name}.option()"),
            _ => format!("value.{name}"),
        };
        assignment(ci, name, &value)?;
    }
    ci.push_str("}) }\nfn mouse_input(value: TuiMouseOptions, button: MouseButton) -> MouseOptions { MouseOptions {\n");
    for f in models.fields("MouseOptions")? {
        assignment(
            ci,
            &f.name,
            &if f.name == "button" {
                "button".into()
            } else {
                format!("value.{}", f.name)
            },
        )?;
    }
    ci.push_str("} }\nunsafe fn recording_input(value: TuiRecordingOptions, format: Option<RecordingFormat>, fps: Option<u8>) -> Result<Operation> { Ok(Operation::StartRecording {\n");
    for f in models.fields("Operation::StartRecording")? {
        let expr = match f.name.as_str() {
            "format" | "fps" => f.name.clone(),
            "background" => {
                "unsafe { capture_background(value.background, value.transparent)? }".into()
            }
            _ if f.ty == "String" => format!("unsafe {{ value.{}.required()? }}", f.name),
            _ => format!("value.{}.option()", f.name),
        };
        assignment(ci, &f.name, &expr)?;
    }
    ci.push_str("}) }\n");
    Ok(())
}

pub fn generate(root: &Path, check: bool) -> Result<()> {
    let models = Models::load(root)?;
    let notice = format!("// {NOTICE}\n");
    let mut c = notice.clone();
    c.push_str(&fs::read_to_string(
        root.join("crates/tui-test-ffi/src/type_helpers.rs"),
    )?);
    let mut cc = notice.clone();
    let mut ci = notice;
    output_models(&models, &mut c, &mut cc)?;
    input_models(&models, &mut c, &mut ci)?;
    input_conversions(&models, &mut ci)?;
    for (path, content) in [
        ("types.rs", c),
        ("output_generated.rs", cc),
        ("input_generated.rs", ci),
        ("session_generated.rs", session_input(&models)?),
    ] {
        emit(
            root,
            &format!("crates/tui-test-ffi/src/{path}"),
            &format_source("rustfmt", &["--edition", "2021"], &content)?,
            check,
        )?;
    }
    javascript::generate(&models, root, check)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn directory() -> tempfile::TempDir {
        let parent = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/bindgen-tests"
        ));
        fs::create_dir_all(parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
    #[test]
    fn owned_fields_drive_c_types_and_conversions() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let mut models = Models::load(root).unwrap();
        models
            .parse("struct Cursor { generation_probe: bool }")
            .unwrap();
        let (mut c, mut cc) = Default::default();
        output_models(&models, &mut c, &mut cc).unwrap();
        assert!(c.contains("pub generation_probe: bool"));
        assert!(cc.contains("generation_probe: v.generation_probe"));
        models
            .parse("struct Cursor { unsupported: Unknown }")
            .unwrap();
        assert!(output_models(&models, &mut c, &mut cc).is_err());
    }
    #[test]
    fn c_v3_input_field_order_remains_fixed() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let models = Models::load(root).unwrap();
        let fields = input_fields(&models, "OpenOptions", false).unwrap();
        let position = |name| fields.iter().position(|f| f.name == name).unwrap();
        assert_eq!(position("env_len"), position("env") + 1);
        assert_eq!(position("scrollback"), position("restart") + 1);
        assert_eq!(position("timeouts"), position("colors_len") + 1);
    }

    #[test]
    fn owned_input_and_context_fields_drive_c_conversions() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let mut models = Models::load(root).unwrap();
        models.parse("struct Timeouts { new_wait: Option<u64> } struct DiagnosticRetentionOptions { new_limit: u16 }").unwrap();
        let (mut c, mut ci) = Default::default();
        input_models(&models, &mut c, &mut ci).unwrap();
        assert!(c.contains("pub not_: bool"));
        assert!(!c.contains("pub not: bool"));
        assert!(c.contains("pub new_wait: TuiOptionalU64"));
        assert!(ci.contains("new_wait: value.new_wait.option()"));
        input_conversions(&models, &mut ci).unwrap();
        assert!(ci.contains("style: Default::default()"));
        assert!(!c.contains("pub style:"));
        let context = session_input(&models).unwrap();
        assert!(context.contains("pub new_limit: Option<u16>"));
        assert!(context.contains("new_limit: self.new_limit.unwrap_or(defaults.new_limit)"));
    }

    #[test]
    fn regeneration_bootstraps_and_checks_every_owned_boundary() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let directory = directory();
        let target = directory.path();
        for path in [
            "crates/tui-test/src/api.rs",
            "crates/tui-test/src/locator_query.rs",
            "crates/tui-test/src/diagnostics.rs",
            "crates/tui-test/src/profile.rs",
            "crates/tui-test/src/shell/mod.rs",
            "crates/tui-test/src/terminal/backend.rs",
            "crates/tui-test/src/terminal/cell.rs",
            "crates/tui-test-ffi/src/type_helpers.rs",
        ] {
            fs::create_dir_all(target.join(path).parent().unwrap()).unwrap();
            fs::copy(root.join(path), target.join(path)).unwrap();
        }
        generate(target, false).unwrap();
        generate(target, true).unwrap();
        for path in [
            "crates/tui-test-ffi/src/types.rs",
            "crates/tui-test-ffi/src/input_generated.rs",
            "crates/tui-test-ffi/src/output_generated.rs",
            "crates/tui-test-ffi/src/session_generated.rs",
            "bindings/js/native/index.d.ts",
            "bindings/js/native/models.js",
        ] {
            let output = target.join(path);
            let original = fs::read_to_string(&output).unwrap();
            fs::write(&output, "// stale\n").unwrap();
            assert!(generate(target, true)
                .unwrap_err()
                .to_string()
                .contains(path));
            fs::write(output, original).unwrap();
        }
    }
}
