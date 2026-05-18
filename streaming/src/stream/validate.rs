// Rejects 200-but-HTML/JSON proxy block-pages: real audio never starts {/[/<.
pub fn looks_like_error_doc(bytes: &[u8]) -> bool {
    let trimmed = trim_ascii_start(bytes);
    matches!(trimmed.first(), Some(b'{') | Some(b'[') | Some(b'<'))
}

pub fn is_valid_audio(bytes: &[u8]) -> bool {
    !bytes.is_empty() && !looks_like_error_doc(bytes)
}

pub fn is_valid_m3u8(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let head_len = bytes.len().min(64);
    String::from_utf8_lossy(&bytes[..head_len]).contains("#EXTM3U")
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    &bytes[i..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_html_and_json_error_pages() {
        assert!(looks_like_error_doc(b"<!DOCTYPE html><html>blocked"));
        assert!(looks_like_error_doc(b"  \n {\"error\":\"forbidden\"}"));
        assert!(looks_like_error_doc(b"[]"));
        assert!(!looks_like_error_doc(b"ID3\x04\x00"));
        assert!(!looks_like_error_doc(&[0xFF, 0xFB, 0x90]));
        assert!(!looks_like_error_doc(b"\x00\x00\x00\x18ftypmp42"));
    }

    #[test]
    fn audio_validator() {
        assert!(!is_valid_audio(b""));
        assert!(!is_valid_audio(b"{\"url\":\"x\"}"));
        assert!(is_valid_audio(b"ID3\x04\x00\x00"));
    }

    #[test]
    fn m3u8_validator() {
        assert!(is_valid_m3u8(b"#EXTM3U\n#EXT-X-VERSION:3\nseg0.ts"));
        assert!(!is_valid_m3u8(b"<html>403</html>"));
        assert!(!is_valid_m3u8(b""));
    }
}
