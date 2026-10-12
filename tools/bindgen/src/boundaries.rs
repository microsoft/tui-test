//! Boundary fields come from owned Rust models. Overrides describe transport differences only.
use crate::{emit, format_source, NOTICE};
use anyhow::{bail, Context, Result};
use quote::ToTokens;
use std::{collections::BTreeMap, fmt::Write, fs, path::Path};
use syn::{Fields, Item};

#[derive(Clone)]
struct Field {
    name: String,
    ty: String,
}

struct Models {
    fields: BTreeMap<String, Vec<Field>>,
    enums: BTreeMap<String, Vec<String>>,
    docs: BTreeMap<String, String>,
}

impl Models {
    fn load(root: &Path) -> Result<Self> {
        let mut models = Self {
            fields: BTreeMap::new(),
            enums: BTreeMap::new(),
            docs: BTreeMap::new(),
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
            let source = fs::read_to_string(root.join(format!("crates/tui-test/src/{module}.rs")))?;
            models.parse(&source)?;
        }
        Ok(models)
    }

    fn parse(&mut self, source: &str) -> Result<()> {
        let fields = |fields: &Fields| -> Result<Vec<Field>> {
            fields
                .iter()
                .map(|f| {
                    Ok(Field {
                        name: f.ident.as_ref().context("named model field")?.to_string(),
                        ty: f.ty.to_token_stream().to_string().replace(' ', ""),
                    })
                })
                .collect()
        };
        for item in syn::parse_file(source)?.items {
            match item {
                Item::Struct(item) if matches!(item.fields, Fields::Named(_)) => {
                    let docs = |attrs: &[syn::Attribute]| {
                        attrs
                            .iter()
                            .filter(|a| a.path().is_ident("doc"))
                            .filter_map(|a| match &a.meta {
                                syn::Meta::NameValue(meta) => match &meta.value {
                                    syn::Expr::Lit(syn::ExprLit {
                                        lit: syn::Lit::Str(text),
                                        ..
                                    }) => Some(
                                        text.value()
                                            .split('\n')
                                            .map(|line| format!("///{line}\n"))
                                            .collect::<String>(),
                                    ),
                                    _ => None,
                                },
                                _ => None,
                            })
                            .collect()
                    };
                    self.docs.insert(item.ident.to_string(), docs(&item.attrs));
                    for f in &item.fields {
                        self.docs.insert(
                            format!("{}.{}", item.ident, f.ident.as_ref().unwrap()),
                            docs(&f.attrs),
                        );
                    }
                    self.fields
                        .insert(item.ident.to_string(), fields(&item.fields)?);
                }
                Item::Enum(item) => {
                    let name = item.ident.to_string();
                    if item
                        .variants
                        .iter()
                        .all(|v| matches!(v.fields, Fields::Unit))
                    {
                        self.enums.insert(
                            name.clone(),
                            item.variants.iter().map(|v| v.ident.to_string()).collect(),
                        );
                    }
                    for v in item.variants {
                        if matches!(v.fields, Fields::Named(_)) {
                            self.fields
                                .insert(format!("{name}::{}", v.ident), fields(&v.fields)?);
                        }
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

fn js_type(ty: &str) -> Result<String> {
    if let Some(inner) = option(ty) {
        return Ok(format!("Option<{}>", js_type(inner)?));
    }
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
        return Ok(format!("Vec<{}>", js_type(inner)?));
    }
    Ok(match ty {
        "u64" | "usize" => "f64".into(),
        "PathBuf" | "WhitespaceMode" | "TraceMode" | "FailureArtifactMode" => "String".into(),
        "CellColor" => "Color".into(),
        "TextStyle" => "LocatorStyle".into(),
        "LocatorDirection" => "LocatorStageDirection".into(),
        "BTreeMap<String,bool>" => "HashMap<String,bool>".into(),
        "std::collections::BTreeMap<u8,String>" => "HashMap<String,String>".into(),
        "(String,String)" => ty.into(),
        t if t.contains("::") => js_type(t.rsplit("::").next().unwrap())?,
        t if matches!(t, "String" | "bool" | "f64" | "u8" | "u16" | "u32" | "i32")
            || OUTPUTS.contains(&t)
            || matches!(
                t,
                "Backend"
                    | "Shell"
                    | "RecordingFormat"
                    | "Timeouts"
                    | "LocatorNode"
                    | "LocatorNodeKind"
            ) =>
        {
            t.into()
        }
        _ => bail!("no N-API mapping for {ty}"),
    })
}

fn js_output(ty: &str, value: &str) -> Result<String> {
    Ok(match ty {
        "u64" | "usize" => format!("{value} as f64"),
        "CellColor" => format!("color({value})"),
        "BTreeMap<String,bool>" => format!("{value}.into_iter().collect()"),
        "std::collections::BTreeMap<u8,String>" => {
            format!("{value}.into_iter().map(|(index,color)| (index.to_string(),color)).collect()")
        }
        "Vec<u8>" => format!("{value}.into()"),
        t if t.starts_with("Vec<") => {
            let inner = &t[4..t.len() - 1];
            format!("{value}.into_iter().map({}).collect()", snake(inner))
        }
        t if OUTPUTS.contains(&t) => format!("{}({value})", snake(t)),
        _ => {
            js_type(ty)?;
            value.into()
        }
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

fn output_models(
    models: &Models,
    c: &mut String,
    js: &mut String,
    cc: &mut String,
    jc: &mut String,
) -> Result<()> {
    for &model in OUTPUTS {
        let fields = models.fields(model)?;
        let c_enabled = !matches!(model, "PackedScreen" | "EffectiveTimeouts");
        if c_enabled {
            writeln!(
                c,
                "#[repr(C)] #[derive(Clone,Copy,Default)] pub struct {} {{",
                c_name(model)
            )?;
        }
        let nullable = matches!(model, "State" | "OpenResult");
        js.push_str(
            models
                .docs
                .get(model)
                .map(String::as_str)
                .unwrap_or_default(),
        );
        if !nullable && model != "Cell" {
            js.push_str("#[derive(Default)]\n");
        }
        writeln!(
            js,
            "#[napi(object{})] pub struct {model} {{",
            if nullable {
                ", use_nullable = true"
            } else {
                ""
            }
        )?;
        if model != "PackedScreen" {
            writeln!(cc, "#[allow(unused_variables)] fn {}(s: &mut Storage, v: tui_test::api::{model}) -> {} {{", snake(model), c_name(model))?;
        }
        writeln!(
            jc,
            "pub fn {}(value: api::{model}) -> {}{model}{} {{",
            snake(model),
            if model == "Cell" { "Result<" } else { "" },
            if model == "Cell" { ">" } else { "" }
        )?;
        writeln!(jc, "{}{model} {{", if model == "Cell" { "Ok(" } else { "" })?;
        let mut init = String::new();
        for f in fields {
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
            if model != "PackedScreen" {
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
            if matches!(model, "OpenResult" | "State" | "Cell" | "BellEvent")
                && f.name.contains('_')
            {
                writeln!(js, "#[napi(js_name = {:?})]", f.name)?;
            }
            js.push_str(
                models
                    .docs
                    .get(&format!("{model}.{}", f.name))
                    .map(String::as_str)
                    .unwrap_or_default(),
            );
            if model == "PackedScreen" {
                writeln!(
                    js,
                    "#[napi(readonly{})]",
                    if f.name == "utf8" {
                        ", ts_type = \"Uint8Array\""
                    } else {
                        ""
                    }
                )?;
            }
            let ty = if model == "Cell" && f.name == "underline_style" {
                "UnderlineStyle".into()
            } else if model == "PackedScreen" && f.name == "utf8" {
                "Buffer".into()
            } else {
                js_type(&f.ty)?
            };
            writeln!(js, "pub {field}: {ty},")?;
            let expr = if model == "Cell" && f.name == "underline_style" {
                "underline_style(&value.underline_style)?".into()
            } else {
                js_output(&f.ty, &format!("value.{field}"))?
            };
            writeln!(jc, "{field}: {expr},")?;
        }
        if c_enabled {
            c.push_str("}\n");
        }
        if model != "PackedScreen" {
            writeln!(cc, "{} {{ {init} }} }}", c_name(model))?;
        }
        writeln!(jc, "}}{} }}", if model == "Cell" { ")" } else { "" })?;
        js.push_str("}\n");
    }
    Ok(())
}

fn enum_models(models: &Models, js: &mut String, jc: &mut String) -> Result<()> {
    for (model, target, path) in [
        ("Backend", "Backend", "terminal::backend"),
        ("Shell", "Shell", "shell"),
        ("RecordingFormat", "RecordingFormat", "api"),
        ("SnapshotResult", "SnapshotResult", "api"),
        ("UnderlineStyle", "UnderlineStyle", "terminal::cell"),
        ("LocatorNodeKind", "LocatorNodeKind", "locator_query"),
        ("LocatorDirection", "LocatorStageDirection", "api"),
    ] {
        let variants = models
            .enums
            .get(model)
            .with_context(|| format!("missing enum {model}"))?;
        writeln!(js, "#[derive(serde::Serialize)] #[serde(rename_all = \"lowercase\")] #[napi(string_enum = \"lowercase\")] pub enum {target} {{ {} }}", variants.join(","))?;
        // UnderlineStyle's owned API field is a string, not the terminal enum.
        if model == "UnderlineStyle" {
            jc.push_str(
                "fn underline_style(value: &str) -> Result<UnderlineStyle> { Ok(match value {\n",
            );
            for variant in variants {
                writeln!(
                    jc,
                    "{:?} => UnderlineStyle::{variant},",
                    variant.to_lowercase()
                )?;
            }
            jc.push_str("value => return Err(internal(format!(\"terminal returned unknown underline style '{value}'\"))), }) }\n");
        } else {
            writeln!(jc, "impl From<{target}> for tui_test::{path}::{model} {{ fn from(value: {target}) -> Self {{ match value {{")?;
            for variant in variants {
                writeln!(jc, "{target}::{variant} => Self::{variant},")?;
            }
            jc.push_str("} } }\n");
            if model == "SnapshotResult" {
                writeln!(jc, "impl From<tui_test::{path}::{model}> for {target} {{ fn from(value: tui_test::{path}::{model}) -> Self {{ match value {{")?;
                for variant in variants {
                    writeln!(
                        jc,
                        "tui_test::{path}::{model}::{variant} => Self::{variant},"
                    )?;
                }
                jc.push_str("} } }\n");
            }
        }
    }
    Ok(())
}

/// Input defaults and flattening are boundary contracts, not new model definitions.
fn input_fields(models: &Models, model: &str, js: bool) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    for mut f in models.fields(model)? {
        match (model, f.name.as_str()) {
            ("OpenOptions" | "RunOptions", "profile") => continue,
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
                                "Option<f64>"
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
                ("OpenOptions" | "RunOptions", "cols" | "rows") => "Option<f64>".into(),
                ("OpenOptions" | "RunOptions", _)
                    if option(&f.ty).is_none() && f.name != "program" =>
                {
                    format!("Option<{}>", js_type(&f.ty)?)
                }
                ("TraceOptions", _) | ("FailureArtifactOptions", "mode" | "include_recording") => {
                    format!("Option<{}>", js_type(&f.ty)?)
                }
                ("Operation::StartRecording", "fps") => "Option<f64>".into(),
                ("LocatorNode", "occurrence") => "Option<String>".into(),
                ("Operation::WaitTitle" | "Operation::Snapshot" | "Operation::Screenshot", _)
                    if f.ty == "bool" =>
                {
                    "Option<bool>".into()
                }
                _ => js_type(&f.ty)?,
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
                ty: "Option<f64>".into(),
            });
        }
    }
    if js && matches!(model, "OpenOptions" | "RunOptions") {
        for mut f in models.fields("DiagnosticRetentionOptions")? {
            if f.ty != "u16" {
                bail!("no retention number mapping for {}", f.ty);
            }
            f.ty = "Option<f64>".into();
            fields.push(f);
        }
    }
    // C v3 field order is fixed. These fields differ from the owned operation order.
    if !js && model == "Operation::WaitTitle" {
        fields.sort_by_key(|f| match f.name.as_str() {
            "timeout_ms" => 0,
            "regex" => 1,
            _ => 2,
        });
    }
    Ok(fields)
}

fn input_models(
    models: &Models,
    c: &mut String,
    js: &mut String,
    jc: &mut String,
    ci: &mut String,
) -> Result<()> {
    for (model, target) in [
        ("Timeouts", "Timeouts"),
        ("OpenOptions", "OpenOptions"),
        ("MouseOptions", "MouseOptions"),
        ("Operation::WaitTitle", "WaitOptions"),
        ("Operation::StartRecording", "RecordingOptions"),
    ] {
        if model == "MouseOptions" {
            c.push_str("/// button: 0 left, 1 middle, 2 right.\n");
        }
        writeln!(
            c,
            "#[repr(C)] #[derive(Clone,Copy,Default)] pub struct Tui{target} {{"
        )?;
        for f in input_fields(models, model, false)? {
            writeln!(c, "pub {}: {},", ident(&f.name), f.ty)?;
        }
        c.push_str("}\n");
    }
    for (model, target) in [
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
    ] {
        let strict = target.starts_with("Locator");
        if !matches!(target, "LocatorNode" | "LocatorExpression") {
            writeln!(
                js,
                "#[derive(Default{})]",
                if target == "LocatorStyle" {
                    ", serde::Serialize"
                } else {
                    ""
                }
            )?;
        }
        writeln!(
            js,
            "#[napi(object{})] pub struct {target} {{",
            if strict {
                ", object_from_js = false"
            } else {
                ""
            }
        )?;
        let mut fields = input_fields(models, model, true)?;
        if target == "ClipboardWaitOptions" {
            fields.retain(|f| f.name != "not");
        }
        for f in &fields {
            writeln!(js, "pub {}: {},", ident(&f.name), f.ty)?;
        }
        js.push_str("}\n");
        if strict {
            writeln!(js, "strict_locator_input!({target} {{")?;
            for f in fields {
                writeln!(js, "{} => {:?},", ident(&f.name), js_key(&f.name))?;
            }
            js.push_str("});\n");
        }
    }
    // Both transports use the same source timeout fields, with different number validation.
    jc.push_str("pub fn timeouts(value: Timeouts) -> Result<api::Timeouts> { Ok(api::Timeouts {\n");
    ci.push_str("fn timeouts(value: TuiTimeouts) -> Timeouts { Timeouts {\n");
    for f in models.fields("Timeouts")? {
        writeln!(
            jc,
            "{}: optional_integer(value.{}, \"timeouts.{}\", u64::MAX)?,",
            f.name, f.name, f.name
        )?;
        writeln!(ci, "{}: value.{}.option(),", f.name, f.name)?;
    }
    jc.push_str("}) }\n");
    ci.push_str("} }\n");
    Ok(())
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

fn input_conversions(models: &Models, jc: &mut String, ci: &mut String) -> Result<()> {
    jc.push_str("fn open_input(value: OpenOptions, profile: Profile, directory: Option<String>) -> Result<api::OpenOptions> { Ok(api::OpenOptions {\n");
    ci.push_str("unsafe fn open_input(value: TuiOpenOptions, backend: tui_test::terminal::backend::Backend, shell: Option<Shell>, profile: Profile, mode: AutomaticRecordingMode) -> Result<OpenOptions> { Ok(OpenOptions {\n");
    for f in models.fields("OpenOptions")? {
        let name = &f.name;
        let js = match name.as_str() {
            "backend" => "value.backend.map(Into::into).unwrap_or_default()".into(),
            "shell" => "value.shell.map(Into::into)".into(),
            "profile" => "profile".into(),
            "cols" => "u16_value(value.cols.unwrap_or(80.0), \"cols\")?".into(),
            "rows" => "u16_value(value.rows.unwrap_or(30.0), \"rows\")?".into(),
            "timeouts" => "timeouts(value.timeouts.unwrap_or_default())?".into(),
            "recording" => "api::AutomaticRecording { mode: api::AutomaticRecordingMode::Disabled, directory: directory.map(Into::into) }".into(),
            _ if option(&f.ty).is_some() => format!("value.{name}"),
            _ => format!("value.{name}.unwrap_or_default()"),
        };
        let c = match name.as_str() {
            "backend" | "shell" | "profile" => name.clone(),
            "cols" => "u16_option(value.cols, \"cols\")?.unwrap_or(80)".into(),
            "rows" => "u16_option(value.rows, \"rows\")?.unwrap_or(30)".into(),
            "env" => "unsafe { pairs(value.env, value.env_len)? }".into(),
            "timeouts" => "timeouts(value.timeouts)".into(),
            "recording" => "AutomaticRecording { mode, directory: unsafe { value.recording_directory.optional()? }.map(Into::into) }".into(),
            _ if f.ty == "Option<String>" => format!("unsafe {{ value.{name}.optional()? }}"),
            _ if option(&f.ty).is_some() => format!("value.{name}.option()"),
            _ => format!("value.{name}"),
        };
        assignment(jc, name, &js)?;
        assignment(ci, name, &c)?;
    }
    jc.push_str("}) }\n");
    ci.push_str("}) }\n");
    for (model, prefix, default_mode) in [
        ("FailureArtifactOptions", "artifact", "all"),
        ("TraceOptions", "trace", "off"),
    ] {
        writeln!(
            jc,
            "pub fn apply_{prefix}(context: &mut crate::session::SessionInput, value: {model}) {{"
        )?;
        for f in models.fields(model)? {
            let expr = match f.ty.as_str() {
                "FailureArtifactMode" | "TraceMode" => format!(
                    "Some(value.{}.unwrap_or_else(|| {default_mode:?}.into()))",
                    f.name
                ),
                "PathBuf" if model == "FailureArtifactOptions" => format!("Some(value.{})", f.name),
                "bool" => format!("value.{}.unwrap_or(false)", f.name),
                _ => format!("value.{}", f.name),
            };
            writeln!(jc, "context.{prefix}_{} = {expr};", f.name)?;
        }
        jc.push_str("}\n");
    }
    for model in ["OpenOptions", "RunOptions"] {
        writeln!(jc, "pub fn {}_context(context: &mut crate::session::SessionInput, value: &{model}) -> Result<()> {{", snake(model))?;
        for f in models.fields("DiagnosticRetentionOptions")? {
            writeln!(
                jc,
                "context.{} = optional_u16(value.{}, {:?})?;",
                f.name,
                f.name,
                js_key(&f.name)
            )?;
        }
        jc.push_str("Ok(()) }\n");
    }
    jc.push_str("pub fn run_options(value: RunOptions, directory: Option<String>) -> Result<api::RunOptions> {\nlet open = open_options(OpenOptions {\n");
    let run = input_fields(models, "RunOptions", true)?;
    for f in input_fields(models, "OpenOptions", true)? {
        if run.iter().any(|r| r.name == f.name) {
            writeln!(jc, "{}: value.{},", f.name, f.name)?;
        }
    }
    jc.push_str("..Default::default() }, directory)?;\nif value.program.is_empty() { return Err(usage(\"program must not be empty\")); }\nOk(api::RunOptions {\n");
    for f in models.fields("RunOptions")? {
        let expr = match f.name.as_str() {
            "program" => "value.program".into(),
            "args" => "value.args.unwrap_or_default()".into(),
            _ => format!("open.{}", f.name),
        };
        writeln!(jc, "{}: {expr},", f.name)?;
    }
    jc.push_str("}) }\n");

    ci.push_str("fn mouse_input(value: TuiMouseOptions, button: MouseButton) -> MouseOptions { MouseOptions {\n");
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
    ci.push_str("} }\n");
    ci.push_str("unsafe fn recording_input(value: TuiRecordingOptions, format: Option<RecordingFormat>, fps: Option<u8>) -> Result<Operation> { Ok(Operation::StartRecording {\n");
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
    for (variant, dto, args) in [
        ("Snapshot", "SnapshotOptions", ", name: String"),
        ("Screenshot", "ScreenshotOptions", ""),
        ("StartRecording", "RecordingOptions", ""),
        ("WaitTitle", "TitleOptions", ", text: String"),
        ("ExpectTitle", "TitleOptions", ", text: String"),
    ] {
        writeln!(jc, "pub fn {}_operation(value: {dto}{args}) -> Result<api::Operation> {{ Ok(api::Operation::{variant} {{", snake(variant))?;
        for f in models.fields(&format!("Operation::{variant}"))? {
            let expr = match f.name.as_str() {
                "name" | "text" => f.name.clone(),
                "background" => "crate::session::capture_background(value.background, value.transparent.unwrap_or(false)).map_err(native_error)?".into(),
                "format" => "value.format.map(Into::into)".into(),
                "fps" => "value.fps.map(|fps| u8_value(fps, \"fps\")).transpose()?".into(),
                "timeout_ms" => "optional_integer(value.timeout_ms, \"timeoutMs\", u64::MAX)?".into(),
                _ if f.ty == "bool" => format!("value.{}.unwrap_or(false)", f.name),
                _ => format!("value.{}", f.name),
            };
            assignment(jc, &f.name, &expr)?;
        }
        jc.push_str("}) }\n");
    }
    jc.push_str("fn locator_style(value: LocatorStyle) -> api::TextStyle { api::TextStyle {\n");
    for f in models.fields("TextStyle")? {
        writeln!(jc, "{}: value.{},", f.name, f.name)?;
    }
    jc.push_str("} }\nfn locator_node(value: LocatorNode, index: usize) -> Result<tui_test::locator_query::LocatorNode> { Ok(tui_test::locator_query::LocatorNode {\n");
    for f in models.fields("LocatorNode")? {
        let expr = match f.ty.as_str() {
            "LocatorNodeKind" => format!("value.{}.into()", f.name),
            "Option<LocatorDirection>" => format!("value.{}.map(Into::into)", f.name),
            "Option<WhitespaceMode>" => format!("value.{}.map(|s| serde_json::from_value(serde_json::Value::String(s)).map_err(|error| usage(format!(\"invalid locator expression: {{error}}\")))).transpose()?", f.name),
            "Option<TextStyle>" => format!("value.{}.map(locator_style)", f.name),
            "Option<usize>" => format!("optional_integer(value.{}, \"locator reference\", 255)?.map(|v| v as usize)", f.name),
            "MatchOccurrence" => "occurrence(value.occurrence, value.nth, index)?".into(),
            _ => format!("value.{}", f.name),
        };
        writeln!(jc, "{}: {expr},", f.name)?;
    }
    jc.push_str("}) }\npub fn query(value: LocatorExpression) -> Result<api::LocatorQuery> { tui_test::locator_query::LocatorExpression {\n");
    for f in models.fields("LocatorExpression")? {
        let expr = match f.ty.as_str() {
            "Vec<LocatorNode>" => format!("value.{}.into_iter().enumerate().map(|(i,v)| locator_node(v,i)).collect::<Result<_>>()?", f.name),
            "usize" => format!("integer(value.{}, \"locator root\", 255)? as usize", f.name),
            _ => bail!("no locator expression conversion for {}", f.ty),
        };
        writeln!(jc, "{}: {expr},", f.name)?;
    }
    jc.push_str("}.into_query().map_err(native_error) }\n");
    Ok(())
}

pub fn generate(root: &Path, check: bool) -> Result<()> {
    let models = Models::load(root)?;
    let notice = format!("// {NOTICE}\n");
    let mut c = notice.clone();
    c.push_str(&fs::read_to_string(
        root.join("crates/tui-test-ffi/src/type_helpers.rs"),
    )?);
    let mut js = format!("{notice}include!(\"type_helpers.rs\");\n");
    let mut cc = notice.clone();
    let mut jc = notice.clone();
    let mut ci = notice;
    output_models(&models, &mut c, &mut js, &mut cc, &mut jc)?;
    enum_models(&models, &mut js, &mut jc)?;
    input_models(&models, &mut c, &mut js, &mut jc, &mut ci)?;
    input_conversions(&models, &mut jc, &mut ci)?;
    for (path, content) in [
        ("types.rs", c),
        ("napi/types.rs", js),
        ("output_generated.rs", cc),
        ("napi/convert_generated.rs", jc),
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
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    pub(crate) fn directory() -> tempfile::TempDir {
        let parent = root().join("target/bindgen-tests");
        fs::create_dir_all(&parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }

    #[test]
    fn owned_field_propagates_to_both_declarations_and_conversions() {
        let mut models = Models::load(root()).unwrap();
        let source = fs::read_to_string(root().join("crates/tui-test/src/api.rs")).unwrap();
        models
            .parse(&source.replace(
                "pub visible: bool,",
                "pub visible: bool, pub generation_probe: bool,",
            ))
            .unwrap();
        let (mut c, mut js, mut cc, mut jc) = Default::default();
        output_models(&models, &mut c, &mut js, &mut cc, &mut jc).unwrap();
        assert!(c.contains("pub generation_probe: bool,"));
        assert!(js.contains("pub generation_probe: bool,"));
        assert!(cc.contains("generation_probe: v.generation_probe,"));
        assert!(jc.contains("generation_probe: value.generation_probe,"));
    }

    #[test]
    fn input_field_propagates_to_both_number_contracts() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("pub struct Timeouts { pub new_wait: Option<u64> }")
            .unwrap();
        let (mut c, mut js, mut jc, mut ci) = Default::default();
        input_models(&models, &mut c, &mut js, &mut jc, &mut ci).unwrap();
        assert!(c.contains("pub new_wait: TuiOptionalU64,"));
        assert!(js.contains("pub new_wait: Option<f64>,"));
        assert!(ci.contains("new_wait: value.new_wait.option(),"));
        assert!(jc.contains("optional_integer(value.new_wait, \"timeouts.new_wait\", u64::MAX)?"));
    }

    #[test]
    fn transport_overrides_preserve_layout_and_javascript_contracts() {
        let models = Models::load(root()).unwrap();
        let (mut c, mut js, mut cc, mut jc) = Default::default();
        output_models(&models, &mut c, &mut js, &mut cc, &mut jc).unwrap();
        assert!(c.contains("pub character: TuiString,"));
        assert!(c.contains("pub spans: *const TuiSpan, pub spans_len: usize,"));
        assert!(js.contains("#[napi(object, use_nullable = true)] pub struct State"));
        assert!(js.contains("#[napi(js_name = \"bell_count\")]"));
        assert!(js.contains("pub bell_count: f64,"));
        assert!(js.contains("pub r#char: String,"));
        assert!(js.contains("/// Whether the cursor is drawn (`DECTCEM`)."));
        assert!(js.contains("pub fg: Color,"));
        assert!(js.contains("#[napi(readonly, ts_type = \"Uint8Array\")]"));
        assert!(cc.contains("let spans = s.spans(v.spans);"));
        assert!(jc.contains("underline_style(&value.underline_style)?"));
        let open = input_fields(&models, "OpenOptions", false).unwrap();
        let position = |name| open.iter().position(|f| f.name == name).unwrap();
        assert_eq!(position("env_len"), position("env") + 1);
        assert_eq!(position("scrollback"), position("restart") + 1);
        assert_eq!(position("timeouts"), position("colors_len") + 1);
    }

    #[test]
    fn strict_locator_keys_follow_owned_fields() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("pub struct TextStyle { pub new_predicate: Option<bool> }")
            .unwrap();
        let (mut c, mut js, mut jc, mut ci) = Default::default();
        input_models(&models, &mut c, &mut js, &mut jc, &mut ci).unwrap();
        assert!(js.contains("pub new_predicate: Option<bool>,"));
        assert!(js.contains("new_predicate => \"newPredicate\","));
        input_conversions(&models, &mut jc, &mut ci).unwrap();
        assert!(jc.contains("new_predicate: value.new_predicate,"));
    }

    #[test]
    fn session_context_fields_follow_diagnostics() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("pub struct DiagnosticRetentionOptions { pub new_limit: u16 }")
            .unwrap();
        let session = session_input(&models).unwrap();
        assert!(session.contains("pub new_limit: Option<u16>,"));
        assert!(session.contains("new_limit: self.new_limit.unwrap_or(defaults.new_limit),"));
        let fields = input_fields(&models, "OpenOptions", true).unwrap();
        assert!(fields
            .iter()
            .any(|f| f.name == "new_limit" && f.ty == "Option<f64>"));
        let (mut jc, mut ci) = Default::default();
        input_conversions(&models, &mut jc, &mut ci).unwrap();
        assert!(jc.contains("context.new_limit = optional_u16(value.new_limit, \"newLimit\")?;"));
    }

    #[test]
    fn enum_variants_follow_owned_source() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("pub enum RecordingFormat { Cast, Future }")
            .unwrap();
        let (mut js, mut jc) = Default::default();
        enum_models(&models, &mut js, &mut jc).unwrap();
        assert!(js.contains("pub enum RecordingFormat { Cast,Future }"));
        assert!(jc.contains("RecordingFormat::Future => Self::Future,"));
    }

    #[test]
    fn unknown_model_types_fail_instead_of_disappearing() {
        let mut models = Models::load(root()).unwrap();
        models
            .parse("pub struct Cursor { pub unknown: Unsupported }")
            .unwrap();
        let (mut c, mut js, mut cc, mut jc) = Default::default();
        assert!(output_models(&models, &mut c, &mut js, &mut cc, &mut jc).is_err());
    }

    #[test]
    fn regeneration_needs_no_generated_rust_and_check_detects_each_output() {
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
            fs::copy(root().join(path), target.join(path)).unwrap();
        }
        generate(target, false).unwrap();
        generate(target, true).unwrap();
        for path in [
            "types.rs",
            "napi/types.rs",
            "output_generated.rs",
            "napi/convert_generated.rs",
            "input_generated.rs",
            "session_generated.rs",
        ] {
            let output = target.join("crates/tui-test-ffi/src").join(path);
            let original = fs::read_to_string(&output).unwrap();
            fs::write(&output, "// stale\n").unwrap();
            let error = generate(target, true).unwrap_err().to_string();
            assert!(error.contains(path), "{error}");
            assert!(error.contains("stale"), "{error}");
            fs::write(output, original).unwrap();
        }
    }
}
