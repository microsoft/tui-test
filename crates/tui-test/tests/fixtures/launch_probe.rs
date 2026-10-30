//! Probe child for launch tests: echoes arguments or checks native bytes.
//!
//! This binary exists because the test harness binary cannot receive
//! non-UTF8 or shell-looking arguments: libtest parses extra arguments as
//! filters and reads them as Unicode. The probe has no harness, so its
//! argument vector reaches the fixture code unchanged.

use std::ffi::OsStr;

const CHECK_HEX: &str = "TUI_TEST_U08_CHECK_HEX";

fn hex_decode(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "hex input must pair digits");
    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16).expect("hex input must be valid")
        })
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        value.to_string_lossy().into_owned().into_bytes()
    }
}

fn main() {
    let mode = std::env::var("TUI_TEST_LAUNCH_PROBE").unwrap_or_default();
    match mode.as_str() {
        "echo-args" => {
            for arg in std::env::args_os().skip(1) {
                println!("ARG:{}", arg.to_string_lossy());
            }
        }
        "check-native-arg" => {
            let expected =
                hex_decode(&std::env::var(CHECK_HEX).expect("native argument check needs hex"));
            let last = std::env::args_os()
                .last()
                .expect("native argument check needs argv");
            if os_bytes(&last) == expected {
                println!("NATIVE-ARG-PASS");
            } else {
                println!("NATIVE-ARG-FAIL got={}", hex_encode(&os_bytes(&last)));
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown launch probe mode: {other}");
            std::process::exit(2);
        }
    }
}
