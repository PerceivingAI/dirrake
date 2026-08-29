use std::{ffi::OsStr, path::Path};

pub fn contains_case_insensitive(value: &OsStr, needle_lower: &str) -> bool {
    if let Some(text) = value.to_str() {
        return text.to_lowercase().contains(needle_lower);
    }

    if !needle_lower.is_ascii() {
        return false;
    }

    contains_ascii_case_insensitive(os_units(value), needle_lower.as_bytes())
}

pub fn eq_case_insensitive(value: &OsStr, expected_lower: &str) -> bool {
    if let Some(text) = value.to_str() {
        return text.to_lowercase() == expected_lower;
    }

    if !expected_lower.is_ascii() {
        return false;
    }

    let units = os_units(value);
    units.len() == expected_lower.len()
        && units
            .iter()
            .zip(expected_lower.bytes())
            .all(|(actual, expected)| ascii_lower(*actual) == u32::from(expected))
}

pub fn normalized_os_text(value: &OsStr) -> String {
    if let Some(text) = value.to_str() {
        return text.to_lowercase();
    }
    format!("{}{}", invalid_prefix(), escape_os_str(value, true))
}

pub fn path_string(path: &Path) -> String {
    os_str_string(path.as_os_str())
}

pub fn terminal_path(path: &Path) -> String {
    visible_controls(&path_string(path))
}

pub fn terminal_os_str(value: &OsStr) -> String {
    visible_controls(&os_str_string(value))
}

pub fn visible_controls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            output.extend(character.escape_default());
        } else {
            output.push(character);
        }
    }
    output
}

pub fn os_str_string(value: &OsStr) -> String {
    match value.to_str() {
        Some(text) => text.to_owned(),
        None => format!("{}{}", invalid_prefix(), escape_os_str(value, false)),
    }
}

fn contains_ascii_case_insensitive(haystack: Vec<u32>, needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle.iter().copied())
            .all(|(actual, expected)| ascii_lower(*actual) == u32::from(expected))
    })
}

fn ascii_lower(value: u32) -> u32 {
    if (u32::from(b'A')..=u32::from(b'Z')).contains(&value) {
        value + u32::from(b'a' - b'A')
    } else {
        value
    }
}

#[cfg(unix)]
fn os_units(value: &OsStr) -> Vec<u32> {
    use std::os::unix::ffi::OsStrExt;
    value
        .as_bytes()
        .iter()
        .map(|byte| u32::from(*byte))
        .collect()
}

#[cfg(windows)]
fn os_units(value: &OsStr) -> Vec<u32> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().map(u32::from).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_units(value: &OsStr) -> Vec<u32> {
    value
        .to_string_lossy()
        .chars()
        .map(|character| character as u32)
        .collect()
}

#[cfg(unix)]
fn escape_os_str(value: &OsStr, lowercase_ascii: bool) -> String {
    use std::os::unix::ffi::OsStrExt;
    escape_unix_bytes(value.as_bytes(), lowercase_ascii)
}

#[cfg(unix)]
fn escape_unix_bytes(bytes: &[u8], lowercase_ascii: bool) -> String {
    let mut output = String::new();
    let mut remaining = bytes;

    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                push_valid_text(&mut output, valid, lowercase_ascii);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid = std::str::from_utf8(&remaining[..valid_up_to])
                        .expect("UTF-8 error valid prefix must decode");
                    push_valid_text(&mut output, valid, lowercase_ascii);
                }

                let invalid_len = error.error_len().unwrap_or(remaining.len() - valid_up_to);
                for byte in &remaining[valid_up_to..valid_up_to + invalid_len] {
                    output.push_str(&format!("\\x{byte:02X}"));
                }
                remaining = &remaining[valid_up_to + invalid_len..];
            }
        }
    }

    output
}

#[cfg(windows)]
fn escape_os_str(value: &OsStr, lowercase_ascii: bool) -> String {
    use std::{char::decode_utf16, os::windows::ffi::OsStrExt};

    let mut output = String::new();
    for decoded in decode_utf16(value.encode_wide()) {
        match decoded {
            Ok(character) => push_character(&mut output, character, lowercase_ascii),
            Err(error) => output.push_str(&format!("\\u{:04X}", error.unpaired_surrogate())),
        }
    }
    output
}

#[cfg(not(any(unix, windows)))]
fn escape_os_str(value: &OsStr, lowercase_ascii: bool) -> String {
    let text = value.to_string_lossy();
    if lowercase_ascii {
        text.to_lowercase()
    } else {
        text.into_owned()
    }
}

#[cfg(unix)]
fn push_valid_text(output: &mut String, text: &str, lowercase_ascii: bool) {
    for character in text.chars() {
        push_character(output, character, lowercase_ascii);
    }
}

#[cfg(any(unix, windows))]
fn push_character(output: &mut String, character: char, lowercase_ascii: bool) {
    if character == '\\' {
        output.push_str("\\\\");
    } else if lowercase_ascii && character.is_ascii() {
        output.push(character.to_ascii_lowercase());
    } else {
        output.push(character);
    }
}

#[cfg(unix)]
fn invalid_prefix() -> &'static str {
    "<non-utf8>:"
}

#[cfg(windows)]
fn invalid_prefix() -> &'static str {
    "<non-unicode>:"
}

#[cfg(not(any(unix, windows)))]
fn invalid_prefix() -> &'static str {
    "<non-unicode>:"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_unicode_text_is_preserved() {
        let value = OsStr::new("Cámara-📷-東京.RS");
        assert!(contains_case_insensitive(value, "cámara"));
        assert_eq!(normalized_os_text(OsStr::new("RS")), "rs");
        assert_eq!(os_str_string(value), "Cámara-📷-東京.RS");
    }

    #[test]
    fn terminal_text_escapes_control_characters() {
        assert_eq!(visible_controls("a\nb\t\u{1b}c"), "a\\nb\\t\\u{1b}c");
    }

    #[cfg(unix)]
    #[test]
    fn unix_invalid_bytes_are_matched_without_lossy_conversion() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let value = OsString::from_vec(b"CAMERA-\xFF.RS".to_vec());
        assert!(contains_case_insensitive(&value, "camera"));
        assert!(!contains_case_insensitive(&value, "other"));
        assert_eq!(os_str_string(&value), "<non-utf8>:CAMERA-\\xFF.RS");
    }
}
