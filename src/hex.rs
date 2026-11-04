use std::fmt::Write;

/// Lowercase hex digits, two per byte.
pub fn lower<'a>(bytes: impl IntoIterator<Item = &'a u8>) -> String {
    bytes.into_iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").ok();
        hex
    })
}
