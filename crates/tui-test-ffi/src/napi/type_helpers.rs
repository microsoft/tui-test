use ::napi::bindgen_prelude::{
    Buffer, Either, FromNapiValue, JsObjectValue, Object, ValidateNapiValue,
};
use napi::{Error, Result, Status};
use std::collections::HashMap;

// Reject unknown fields before conversion so misspelled predicates cannot
// silently broaden a locator. The generator supplies the field names.
macro_rules! strict_locator_input {
    ($name:ident { $($field:ident => $key:literal),* $(,)? }) => {
        impl ValidateNapiValue for $name {}
        impl FromNapiValue for $name {
            unsafe fn from_napi_value(env: napi::sys::napi_env, value: napi::sys::napi_value) -> Result<Self> {
                unsafe { Object::validate(env, value)?; }
                let object = unsafe { Object::from_napi_value(env, value)? };
                for key in Object::keys(&object)? {
                    if ![$($key),*].contains(&key.as_str()) {
                        return Err(Error::new(Status::InvalidArg, format!("unknown locator field {key:?}")));
                    }
                }
                Ok(Self { $($field: object.get_named_property($key)?),* })
            }
        }
    };
}
