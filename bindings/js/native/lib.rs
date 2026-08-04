#![deny(clippy::all)]

use napi::bindgen_prelude::{spawn_blocking, ToNapiValue, TypeName};
use napi::{sys, Error, Result, Status, ValueType};
use napi_derive::napi;
use serde_json::Value;
use shell_use::runtime::global_registry;

fn internal_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::new(Status::GenericFailure, format!("{context}: {error}"))
}

// `Task::JsValue` requires `TypeName`, which `serde_json::Value` lacks.
pub struct JsonValue(Value);

impl TypeName for JsonValue {
    fn type_name() -> &'static str {
        "unknown"
    }

    fn value_type() -> ValueType {
        ValueType::Unknown
    }
}

impl ToNapiValue for JsonValue {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        unsafe { Value::to_napi_value(env, val.0) }
    }
}

#[napi]
pub struct NativeSession {
    name: String,
}

#[napi]
impl NativeSession {
    #[napi(constructor)]
    pub fn new(name: String) -> Self {
        NativeSession { name }
    }

    #[napi]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[napi(ts_return_type = "Promise<unknown>")]
    pub async fn request(&self, payload: Value) -> Result<JsonValue> {
        let name = self.name.clone();
        let output = spawn_blocking(move || {
            let response = global_registry().response_value(&name, payload);
            serde_json::to_value(response)
                .map_err(|error| internal_error("failed to encode shell-use response", error))
        })
        .await
        .map_err(|error| internal_error("native request task failed", error))??;
        Ok(JsonValue(output))
    }
}

#[napi]
pub async fn sessions() -> Result<Vec<String>> {
    spawn_blocking(|| global_registry().sessions())
        .await
        .map_err(|error| internal_error("native sessions task failed", error))
}

#[napi]
pub async fn close_all() -> Result<()> {
    spawn_blocking(|| {
        global_registry().close_all();
    })
    .await
    .map_err(|error| internal_error("native close task failed", error))
}

#[napi]
pub fn close_all_sync() {
    global_registry().close_all();
}

#[napi]
pub async fn recording(name: String) -> Result<String> {
    spawn_blocking(move || {
        global_registry().recording(&name).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    Status::GenericFailure,
                    format!("no recording for session '{name}'"),
                )
            } else {
                internal_error(
                    &format!("failed to read the recording for session '{name}'"),
                    error,
                )
            }
        })
    })
    .await
    .map_err(|error| internal_error("native recording task failed", error))?
}
