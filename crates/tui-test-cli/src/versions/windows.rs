use serde_json::{json, Value};
use tui_test::terminal::conpty;

pub(super) fn report(package: &str) -> Value {
    json!({
        "version": conpty::package_version(),
        "file_version": conpty::file_version(),
        "source": format!("bundled {package}"),
    })
}
