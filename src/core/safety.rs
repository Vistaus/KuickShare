//! Checks on what an unknown sender asks us to do, made before the user (or a trusted-device rule)
//! accepts a transfer. `rqs_lib` puts the file names it is given straight into the download folder.

/// The longest file name most file systems take, in bytes.
const MAX_NAME_BYTES: usize = 255;

/// True if `name` could write outside the download folder or is not a plain file name.
pub fn unsafe_file_name(name: &str) -> bool {
    name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > MAX_NAME_BYTES
        || name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control() || is_invisible(c))
}

/// Zero-width and text-direction characters: with them `photo<U+202E>gpj.exe` shows as `photoexe.jpg`.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_are_fine() {
        for name in [
            "photo.jpg",
            "Video 6.mp4",
            "relazione finale (2).pdf",
            ".bashrc",
            "a.b.c",
        ] {
            assert!(!unsafe_file_name(name), "{name}");
        }
    }

    #[test]
    fn paths_and_odd_names_are_not() {
        let long = "x".repeat(MAX_NAME_BYTES + 1);
        for name in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "/etc/passwd",
            "..\\x",
            "a\0b",
            "line\nbreak",
            "photo\u{202E}gpj.exe",
            "zero\u{200B}width",
            long.as_str(),
        ] {
            assert!(unsafe_file_name(name), "{name:?}");
        }
    }
}
