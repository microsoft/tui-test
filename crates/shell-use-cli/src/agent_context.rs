//! Machine-readable description of the full CLI surface, generated from the
//! clap command model so it can never drift from the real implementation.
//! Printed by `shell-use agent-context`.

use clap::{Arg, ArgAction, Command, CommandFactory};
use serde_json::{json, Map, Value};

use crate::cli::Cli;

const SCHEMA_VERSION: &str = "1";

/// Render the agent-context document as pretty JSON.
pub fn render() -> String {
    let cmd = Cli::command();

    let mut global_flags = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_global_set() {
            global_flags.push(arg_json(arg));
        }
    }

    let doc = json!({
        "schema_version": SCHEMA_VERSION,
        "name": cmd.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "about": cmd.get_about().map(|s| s.to_string()),
        "exit_codes": exit_codes(),
        "global_flags": global_flags,
        "commands": commands_json(&cmd),
    });

    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

fn exit_codes() -> Value {
    json!({
        "0": "success",
        "1": "assertion or wait condition not met",
        "2": "usage / invalid argument",
        "3": "no active session (run `open` or `run` first)",
        "4": "daemon or IPC error",
        "5": "internal error",
    })
}

/// Build a `{ name: {...} }` map of a command's visible subcommands.
fn commands_json(cmd: &Command) -> Value {
    let mut map = Map::new();
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        map.insert(sub.get_name().to_string(), command_json(sub));
    }
    Value::Object(map)
}

fn command_json(cmd: &Command) -> Value {
    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| !a.is_global_set() && !a.is_hide_set())
        .map(arg_json)
        .collect();

    let mut obj = Map::new();
    if let Some(about) = cmd.get_about() {
        obj.insert("about".into(), json!(about.to_string()));
    }
    if !args.is_empty() {
        obj.insert("args".into(), Value::Array(args));
    }
    let subs = commands_json(cmd);
    if subs.as_object().map(|m| !m.is_empty()).unwrap_or(false) {
        obj.insert("subcommands".into(), subs);
    }
    Value::Object(obj)
}

fn arg_json(arg: &Arg) -> Value {
    let positional = arg.is_positional();
    let is_flag = matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse);

    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|pv| pv.get_name().to_string())
        .collect();

    let kind = if is_flag {
        "bool"
    } else if !values.is_empty() {
        "enum"
    } else if positional {
        "value"
    } else {
        "string"
    };

    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();

    let mut obj = Map::new();
    obj.insert("name".into(), json!(arg.get_id().as_str()));
    obj.insert("type".into(), json!(kind));
    if positional {
        obj.insert("positional".into(), json!(true));
    }
    if let Some(long) = arg.get_long() {
        obj.insert("long".into(), json!(format!("--{long}")));
    }
    if let Some(short) = arg.get_short() {
        obj.insert("short".into(), json!(format!("-{short}")));
    }
    if let Some(help) = arg.get_help() {
        obj.insert("help".into(), json!(help.to_string()));
    }
    if !values.is_empty() {
        obj.insert("values".into(), json!(values));
    }
    if !defaults.is_empty() {
        let default = if defaults.len() == 1 {
            json!(defaults[0])
        } else {
            json!(defaults)
        };
        obj.insert("default".into(), default);
    }
    if arg.is_required_set() {
        obj.insert("required".into(), json!(true));
    }
    if matches!(arg.get_action(), ArgAction::Append) {
        obj.insert("repeatable".into(), json!(true));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_versioned_schema_with_commands() {
        let doc: Value = serde_json::from_str(&render()).unwrap();
        assert_eq!(doc["schema_version"], json!(SCHEMA_VERSION));
        assert_eq!(doc["name"], json!("shell-use"));
        assert!(doc["commands"]["expect"].is_object());
        assert!(doc["commands"].get("__daemon").is_none());
        assert!(doc["exit_codes"]["3"].is_string());
        let globals = doc["global_flags"].as_array().unwrap();
        assert!(globals.iter().any(|f| f["long"] == json!("--session")));
    }

    #[test]
    fn enum_args_enumerate_their_values() {
        let doc: Value = serde_json::from_str(&render()).unwrap();
        let signal = &doc["commands"]["signal"]["args"][0];
        assert_eq!(signal["type"], json!("enum"));
        let values = signal["values"].as_array().unwrap();
        assert!(values.contains(&json!("INT")));
    }
}
