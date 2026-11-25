//! A file path inside a link, percent-encoded byte by byte.

pub fn encode(path: &str) -> String {
    let hex_digit = |half: u8| {
        char::from_digit(u32::from(half), 16).map_or('0', |digit| digit.to_ascii_uppercase())
    };
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            _ => {
                encoded.push('%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0F));
            }
        }
    }
    encoded
}

pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn a_path_is_encoded_byte_by_byte_in_upper_case_hex() {
        assert_eq!(
            encode("app/my shop/Café_1~.php"),
            "app/my%20shop/Caf%C3%A9_1~.php"
        );
    }
}
