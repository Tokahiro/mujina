//! UTF-16 string conversion.

/// NUL-terminated.
pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Up to the first NUL, or the whole buffer without one.
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let wide = to_wide("Größe");
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(from_wide(&wide), "Größe");
        assert_eq!(from_wide(&[0x41, 0x42]), "AB");
    }
}
