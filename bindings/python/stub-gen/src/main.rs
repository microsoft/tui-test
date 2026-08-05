mod stubs {
    use std::any::TypeId;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use pyo3_stub_gen::type_info::{MemberInfo, PyClassInfo};
    use pyo3_stub_gen::TypeInfo;

    struct NativeSession;
    struct NativeAssertionError;
    struct NativeUsageError;
    struct NativeNoSessionError;
    struct NativeInternalError;

    fn builtins_exception() -> TypeInfo {
        TypeInfo::builtin("Exception")
    }

    fn builtins_str() -> TypeInfo {
        TypeInfo::builtin("str")
    }

    pyo3_stub_gen::inventory::submit! {
        PyClassInfo {
            struct_id: TypeId::of::<NativeSession>,
            pyclass_name: "NativeSession",
            module: Some("shell_use._native"),
            doc: "",
            getters: &[MemberInfo {
                name: "name",
                r#type: builtins_str,
                doc: "",
                default: None,
                deprecated: None,
            }],
            setters: &[],
            bases: &[],
            has_eq: false,
            has_ord: false,
            has_hash: false,
            has_str: false,
            subclass: false,
        }
    }

    macro_rules! submit_exception_stub {
        ($exception:ty, $name:literal, $doc:literal) => {
            pyo3_stub_gen::inventory::submit! {
                PyClassInfo {
                    struct_id: TypeId::of::<$exception>,
                    pyclass_name: $name,
                    module: Some("shell_use._native"),
                    doc: $doc,
                    getters: &[],
                    setters: &[],
                    bases: &[builtins_exception],
                    has_eq: false,
                    has_ord: false,
                    has_hash: false,
                    has_str: false,
                    subclass: true,
                }
            }
        };
    }

    submit_exception_stub!(
        NativeAssertionError,
        "NativeAssertionError",
        "Native assertion failure."
    );
    submit_exception_stub!(NativeUsageError, "NativeUsageError", "Native usage error.");
    submit_exception_stub!(
        NativeNoSessionError,
        "NativeNoSessionError",
        "Native session was not found."
    );
    submit_exception_stub!(
        NativeInternalError,
        "NativeInternalError",
        "Native internal error."
    );

    pyo3_stub_gen::inventory::submit! {
        pyo3_stub_gen_derive::gen_methods_from_python! {
            r#"
            import typing

            class NativeSession:
                def __new__(cls, name: str) -> NativeSession: ...

                def open(
                    self,
                    shell: typing.Optional[str],
                    cols: int,
                    rows: int,
                    cwd: typing.Optional[str],
                    env: typing.List[typing.Tuple[str, str]],
                    wait_ready: typing.Optional[bool],
                    text_timeout: typing.Optional[int],
                    idle_timeout: typing.Optional[int],
                    command_timeout: typing.Optional[int],
                    exit_timeout: typing.Optional[int],
                    ready_timeout: typing.Optional[int],
                ) -> typing.Awaitable[typing.Dict[str, typing.Any]]: ...

                def run(
                    self,
                    program: str,
                    args: typing.List[str],
                    cols: int,
                    rows: int,
                    cwd: typing.Optional[str],
                    env: typing.List[typing.Tuple[str, str]],
                    wait_ready: typing.Optional[bool],
                    text_timeout: typing.Optional[int],
                    idle_timeout: typing.Optional[int],
                    command_timeout: typing.Optional[int],
                    exit_timeout: typing.Optional[int],
                    ready_timeout: typing.Optional[int],
                ) -> typing.Awaitable[typing.Dict[str, typing.Any]]: ...

                def close(self) -> typing.Awaitable[None]: ...
                def state(self) -> typing.Awaitable[typing.Dict[str, typing.Any]]: ...
                def text(self, full: bool) -> typing.Awaitable[str]: ...
                def packed_screen(self, full: bool) -> typing.Awaitable[typing.Tuple[memoryview, int, int]]:
                    """Return immutable UTF-8 logical rows plus cell dimensions."""
                def cells(self, x: int, y: int, w: int, h: int) -> typing.Awaitable[typing.List[typing.Dict[str, typing.Any]]]: ...
                def get_command(self) -> typing.Awaitable[typing.Optional[str]]: ...
                def get_output(self) -> typing.Awaitable[typing.Optional[str]]: ...
                def get_exit_code(self) -> typing.Awaitable[typing.Optional[int]]: ...
                def get_cwd(self) -> typing.Awaitable[typing.Optional[str]]: ...
                def get_cursor(self) -> typing.Awaitable[typing.Dict[str, int]]: ...
                def get_size(self) -> typing.Awaitable[typing.Dict[str, int]]: ...
                def get_bell_count(self) -> typing.Awaitable[int]: ...
                def write(self, data: str) -> typing.Awaitable[None]: ...
                def type(self, text: str) -> typing.Awaitable[None]: ...
                def submit(self, data: typing.Optional[str]) -> typing.Awaitable[None]: ...
                def press(self, keys: typing.List[str]) -> typing.Awaitable[None]: ...
                def keys(self, combo: str) -> typing.Awaitable[None]: ...
                def mouse_click(
                    self,
                    x: typing.Optional[int],
                    y: typing.Optional[int],
                    on_text: typing.Optional[str],
                    button: int,
                    clicks: int,
                ) -> typing.Awaitable[None]: ...
                def mouse_move(self, x: int, y: int) -> typing.Awaitable[None]: ...
                def mouse_down(self, x: int, y: int, button: int) -> typing.Awaitable[None]: ...
                def mouse_up(self, x: int, y: int, button: int) -> typing.Awaitable[None]: ...
                def mouse_drag(self, x1: int, y1: int, x2: int, y2: int, button: int) -> typing.Awaitable[None]: ...
                def mouse_scroll(self, direction: str, amount: int) -> typing.Awaitable[None]: ...
                def resize(self, cols: int, rows: int) -> typing.Awaitable[None]: ...
                def signal(self, signal: str) -> typing.Awaitable[None]: ...
                def kill(self) -> typing.Awaitable[None]: ...
                def wait_text(
                    self,
                    text: str,
                    regex: bool,
                    full: bool,
                    not_: bool,
                    timeout_ms: typing.Optional[int],
                ) -> typing.Awaitable[None]: ...
                def wait_idle(self, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def wait_command(self, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def wait_exit(self, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def wait_ready(self, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def wait_bell(self, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def expect_text(
                    self,
                    text: str,
                    regex: bool,
                    full: bool,
                    strict: bool,
                    not_: bool,
                    fg: typing.Optional[str],
                    bg: typing.Optional[str],
                    timeout_ms: typing.Optional[int],
                ) -> typing.Awaitable[None]: ...
                def expect_exit_code(self, code: int, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def expect_output(self, text: str, regex: bool) -> typing.Awaitable[None]: ...
                def expect_bell_count(self, count: int, timeout_ms: typing.Optional[int]) -> typing.Awaitable[None]: ...
                def snapshot(
                    self,
                    name: str,
                    update: bool,
                    include_colors: bool,
                    cwd: typing.Optional[str],
                ) -> typing.Awaitable[str]: ...
                def screenshot(self, path: typing.Optional[str], full: bool) -> typing.Awaitable[str]: ...
                def recording(self) -> typing.Awaitable[str]: ...
            "#
        }
    }

    macro_rules! submit_function_stub {
        ($source:literal) => {
            pyo3_stub_gen::inventory::submit! {
                pyo3_stub_gen_derive::gen_function_from_python! {
                    module = "shell_use._native",
                    $source
                }
            }
        };
    }

    submit_function_stub!(
        r#"
        import typing
        def sessions() -> typing.Awaitable[typing.List[str]]: ...
        "#
    );
    submit_function_stub!(
        r#"
        import typing
        def close_all() -> typing.Awaitable[None]: ...
        "#
    );
    submit_function_stub!(
        r#"
        import typing
        def recording(name: str) -> typing.Awaitable[str]: ...
        "#
    );
    submit_function_stub!(
        r#"
        import typing
        def panic_probe() -> typing.Awaitable[None]: ...
        "#
    );
    submit_function_stub!(
        r#"
        def _close_all_blocking() -> None: ...
        "#
    );

    pub fn run(check_only: bool) -> Result<(), Box<dyn Error>> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let pyproject = manifest_dir.join("../pyproject.toml");
        let destination = manifest_dir.join("../src/shell_use/_native.pyi");
        let stub_info = pyo3_stub_gen::StubInfo::from_pyproject_toml(pyproject)?;
        let module = stub_info.modules.get("shell_use._native").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "missing shell_use._native stub metadata",
            )
        })?;
        let generated = format!(
            "{}\n",
            module
                .format_with_config(stub_info.config.use_type_statement)
                .trim_end()
        );

        if check_only {
            check(&destination, &generated)?;
        } else {
            fs::write(&destination, generated)?;
        }

        Ok(())
    }

    fn check(destination: &Path, generated: &str) -> Result<(), Box<dyn Error>> {
        let current = fs::read_to_string(destination)?;
        if current != generated {
            return Err(format!(
                "{} is out of date; run `python scripts/generate_stubs.py` from bindings/python",
                destination.display()
            )
            .into());
        }
        Ok(())
    }
}

fn main() {
    let check_only = std::env::args().skip(1).any(|arg| arg == "--check");
    if let Err(error) = stubs::run(check_only) {
        panic!("{error}");
    }
}
