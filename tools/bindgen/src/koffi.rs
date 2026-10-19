//! Koffi layouts and typed asynchronous wrappers come from the C function signatures.
use super::*;

fn ffi_type(ty: &Type) -> Result<String> {
    if matches!(ty, Type::Ptr(_)) {
        return Ok("void *".into());
    }
    Ok(match name(ty)?.as_str() {
        "u8" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "u64" => "uint64_t",
        "i32" => "int32_t",
        "usize" => "size_t",
        "f64" => "double",
        "bool" => "bool",
        other if other.starts_with("Tui") => return Ok(other.into()),
        other => bail!("unsupported Koffi type: {other}"),
    }
    .into())
}
fn type_reference(ty: &Type) -> Result<String> {
    let ty = ffi_type(ty)?;
    Ok(if ty.starts_with("Tui") {
        format!("types.{ty}")
    } else {
        format!("{ty:?}")
    })
}
fn descriptor(ty: &Type) -> Result<String> {
    if let Type::Ptr(pointer) = ty {
        return Ok(format!("*{}", name(&pointer.elem)?));
    }
    name(ty)
}
fn asynchronous(function: &ItemFn) -> bool {
    matches!(&function.sig.output, ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Ptr(_)))
        && !function.sig.ident.to_string().ends_with("_ptr")
}

pub fn generate(
    root: &Path,
    structs: &[&ItemStruct],
    functions: &[&ItemFn],
    version: &str,
    check: bool,
) -> Result<()> {
    let mut js = format!("// {NOTICE}\nimport koffi from 'koffi';\nexport const abiVersion = {version};\nexport const types = {{}};\n");
    let mut layouts = String::from("export const layouts = {\n");
    let mut emitted = BTreeSet::new();
    while emitted.len() < structs.len() {
        let before = emitted.len();
        for item in structs {
            let id = item.ident.to_string();
            if emitted.contains(&id) {
                continue;
            }
            let ready = item.fields.iter().all(|f| {
                !matches!(&f.ty, Type::Path(_))
                    || name(&f.ty).is_ok_and(|n| !n.starts_with("Tui") || emitted.contains(&n))
            });
            if !ready {
                continue;
            }
            writeln!(js, "types.{id} = koffi.struct({{")?;
            writeln!(layouts, "{id}: {{")?;
            for field in &item.fields {
                let field_name = field.ident.as_ref().context("named C field")?.to_string();
                writeln!(js, "{field_name}: {},", type_reference(&field.ty)?)?;
                writeln!(layouts, "{field_name}: {:?},", descriptor(&field.ty)?)?;
            }
            js.push_str("});\n");
            layouts.push_str("},\n");
            emitted.insert(id);
        }
        if emitted.len() == before {
            bail!("cyclic Koffi structure");
        }
    }
    layouts.push_str("};\n");
    js.push_str(&layouts);
    js.push_str("export function bind(library) {\nconst functions = {};\n");
    let mut wrappers = format!("// {NOTICE}\nuse super::*;\n");
    for function in functions {
        let id = function.sig.ident.to_string();
        let args = arguments(function)?
            .into_iter()
            .map(type_reference)
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        let result = match &function.sig.output {
            ReturnType::Default => "\"void\"".into(),
            ReturnType::Type(_, ty) => type_reference(ty)?,
        };
        writeln!(
            js,
            "functions.{id} = library.func({id:?}, {result}, [{args}]);"
        )?;
        if asynchronous(function) {
            writeln!(
                js,
                "functions.{id}_start = library.func({:?}, 'void *', [{args}]);",
                format!("{id}_start")
            )?;
            let params = function.sig.inputs.to_token_stream().to_string();
            let names = function
                .sig
                .inputs
                .iter()
                .map(|arg| match arg {
                    FnArg::Typed(arg) => Ok(arg.pat.to_token_stream().to_string()),
                    _ => bail!("C receiver"),
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            let call = if function.sig.unsafety.is_some() {
                format!("unsafe {{ {id}({names}) }}")
            } else {
                format!("{id}({names})")
            };
            writeln!(wrappers, "#[no_mangle]\n/// Start a typed call after copying all borrowed inputs.\n/// # Safety\n/// Inputs follow the synchronous function contract through this call only.\npub unsafe extern \"C\" fn {id}_start({params}) -> *mut TuiTask {{ tasks::start(|| {call}) }}")?;
        }
    }
    let tasks = syn::parse_file(&fs::read_to_string(
        root.join("crates/tui-test-ffi/src/tasks.rs"),
    )?)?;
    for item in tasks.items {
        if let Item::Fn(function) = item {
            if !function
                .attrs
                .iter()
                .any(|a| a.path().is_ident("no_mangle"))
            {
                continue;
            }
            let id = function.sig.ident.to_string();
            let args = arguments(&function)?
                .into_iter()
                .map(type_reference)
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            let result = match &function.sig.output {
                ReturnType::Default => "\"void\"".into(),
                ReturnType::Type(_, ty) => type_reference(ty)?,
            };
            writeln!(
                js,
                "functions.{id} = library.func({id:?}, {result}, [{args}]);"
            )?;
        }
    }
    js.push_str("return functions;\n}\n");
    emit(root, "bindings/js/native/abi.js", &js, check)?;
    emit(
        root,
        "crates/tui-test-ffi/src/tasks_generated.rs",
        &format_source("rustfmt", &["--edition", "2021"], &wrappers)?,
        check,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_types_drive_koffi_layouts() {
        assert_eq!(
            ffi_type(&syn::parse_str("u64").unwrap()).unwrap(),
            "uint64_t"
        );
        assert_eq!(
            descriptor(&syn::parse_str("*const TuiCell").unwrap()).unwrap(),
            "*TuiCell"
        );
        assert!(ffi_type(&syn::parse_str("String").unwrap()).is_err());
        assert_eq!(
            type_reference(&syn::parse_str("TuiString").unwrap()).unwrap(),
            "types.TuiString"
        );
        assert_eq!(
            type_reference(&syn::parse_str("*mut TuiResult").unwrap()).unwrap(),
            "\"void *\""
        );
    }
}
