//! Output formatting utilities supporting human-readable and JSON modes.

use serde::Serialize;
use std::fmt::Display;

/// Output a serializable item as pretty JSON or via a custom human-readable closure.
pub fn output_value<T: Serialize>(json: bool, val: &T, human_fmt: impl FnOnce(&T)) {
    if json {
        if let Ok(serialized) = serde_json::to_string_pretty(val) {
            println!("{serialized}");
        }
    } else {
        human_fmt(val);
    }
}

/// Print an informational message (suppressed in JSON mode unless error).
pub fn print_info(json: bool, msg: impl Display) {
    if !json {
        println!("{msg}");
    }
}

/// Print an error message to stderr.
pub fn print_error(msg: impl Display) {
    eprintln!("error: {msg}");
}
