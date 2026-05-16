//! Body validators — used to decide whether a fetched response is a *valid
//! logical result* (parsed track / playlist / real audio bytes), not just an
//! HTTP 2xx. A banned proxy frequently answers 200 with an HTML block page or
//! a tiny JSON error; accepting that as "the winner" is the main source of
//! flaky streams. The race only treats a source as having won once its body
//! passes the relevant validator here.

/// Leading bytes of an HTML/JSON/XML document. Real audio (MP3 `ID3`/frame
/// sync, MP4 `ftyp`, fMP4 segments, MPEG-TS `0x47`) never starts with these,
/// so this reliably rejects proxy block-pages and JSON error payloads served
/// in place of media.
pub fn looks_like_error_doc(bytes: &[u8]) -> bool {
    let trimmed = trim_ascii_start(bytes);
    matches!(trimmed.first(), Some(b'{') | Some(b'[') | Some(b'<'))
}

/// Valid media payload: non-empty and not an error document.
pub fn is_valid_audio(bytes: &[u8]) -> bool {
    !bytes.is_empty() && !looks_like_error_doc(bytes)
}

/// Valid HLS playlist: must carry the `#EXTM3U` tag. A block-page or JSON
/// error never does.
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
