//! Growing byte buffer behind a cache-miss download.
//!
//! `track_cache` tees every chunk it receives into a `StreamBuffer` while it
//! writes the same bytes to the `.part` cache file, so the played audio and the
//! cached file never diverge. The engine hands a `StreamReader` over that
//! buffer to rodio: reads past the buffered end block until more bytes land, the
//! download finishes, or the session is cancelled (track switch, stop, or a
//! container that turned out to need the whole file).
//!
//! The decoder only blocks when it catches up with the network, so a slow
//! connection degrades to plain buffering instead of failing.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Defensive wait slice: every state change notifies, the timeout only keeps a
/// reader from parking forever if a writer died without notifying.
const WAIT_SLICE: Duration = Duration::from_millis(100);

struct Inner {
    data: Vec<u8>,
    /// `Content-Length` of the mirrored response, when the origin sent one.
    /// Doubles as the logical stream size for the decoder (duration/seek scale).
    total: Option<u64>,
    /// No more bytes will land (download returned, failed, or the session ended).
    finished: bool,
    /// Still mirroring the download into memory.
    tee: bool,
    /// Readers handed out so far. A retried response must not reset the bytes
    /// out from under a decoder that already started.
    readers: usize,
    /// A response currently owns the mirror.
    claimed: bool,
    /// The winning response finished writing; nobody may claim again.
    sealed: bool,
    /// Bumped per claim so a released/dropped claim cannot touch later bytes.
    generation: u64,
    quality: Option<&'static str>,
    source: Option<&'static str>,
}

/// Snapshot of the mirror, for the command that decides when to start.
#[derive(Clone, Copy)]
pub struct StreamState {
    pub bytes: u64,
    pub total: Option<u64>,
    pub finished: bool,
}

pub struct StreamBuffer {
    inner: Mutex<Inner>,
    wake: Condvar,
}

impl StreamBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                data: Vec::new(),
                total: None,
                finished: false,
                tee: true,
                readers: 0,
                claimed: false,
                sealed: false,
                generation: 0,
                quality: None,
                source: None,
            }),
            wake: Condvar::new(),
        })
    }

    /// Take the mirror for one response. Only one attempt can hold it at a time:
    /// the direct/stream race starts two downloads, and gluing their bytes
    /// together would corrupt both the audio and the cache metadata. A dropped or
    /// failed claim releases everything it wrote so the next attempt starts clean.
    pub fn claim(self: &Arc<Self>) -> Option<StreamClaim> {
        let mut inner = self.lock();
        if !inner.tee || inner.finished || inner.claimed || inner.sealed || inner.readers > 0 {
            return None;
        }
        inner.claimed = true;
        inner.generation += 1;
        Some(StreamClaim {
            buffer: self.clone(),
            generation: inner.generation,
            committed: false,
        })
    }

    fn abandon(&self, generation: u64) {
        let mut inner = self.lock();
        if inner.generation != generation {
            return;
        }
        inner.claimed = false;
        inner.data.clear();
        drop(inner);
        self.wake.notify_all();
    }

    /// The download returned (success or failure): readers get EOF at whatever
    /// landed instead of waiting for bytes that will never arrive.
    pub fn finish(&self) {
        let mut inner = self.lock();
        inner.finished = true;
        drop(inner);
        self.wake.notify_all();
    }

    /// Give up early playback and keep only the file download (non-progressive
    /// container or a failed player build).
    pub fn detach(&self) {
        let mut inner = self.lock();
        inner.finished = true;
        inner.tee = false;
        inner.data.clear();
        inner.data.shrink_to_fit();
        drop(inner);
        self.wake.notify_all();
    }

    /// Teardown: unblock every reader with EOF and release the mirrored bytes.
    pub fn cancel(&self) {
        self.detach();
    }

    pub fn state(&self) -> StreamState {
        let inner = self.lock();
        StreamState {
            bytes: inner.data.len() as u64,
            total: inner.total,
            finished: inner.finished,
        }
    }

    /// Logical size for the decoder: the advertised `Content-Length` even while
    /// the body is still arriving, or the received length once it is complete.
    ///
    /// ISO-BMFF gets `None` on purpose: declared seekable, symphonia walks every
    /// top-level atom to find the last segment index, which parks a growing
    /// stream at its unbuffered end until the download finishes. Without a length
    /// it decodes both fragmented and front-moov files incrementally; seeking is
    /// restored when `finalize_stream` adopts the finished bytes.
    pub fn byte_len(&self) -> Option<u64> {
        let inner = self.lock();
        if is_mp4_header(&inner.data) {
            return None;
        }
        inner.total.or(if inner.finished {
            Some(inner.data.len() as u64)
        } else {
            None
        })
    }

    pub fn quality(&self) -> Option<&'static str> {
        self.lock().quality
    }

    pub fn source(&self) -> Option<&'static str> {
        self.lock().source
    }

    /// First `max` received bytes, for the container sniff that decides whether
    /// early playback is possible at all.
    pub fn head(&self, max: usize) -> Vec<u8> {
        let inner = self.lock();
        inner.data[..inner.data.len().min(max)].to_vec()
    }

    pub fn copy_bytes(&self) -> Vec<u8> {
        self.lock().data.clone()
    }

    /// A decoder reads through its own reader: position lives in the reader, the
    /// bytes stay shared.
    pub fn reader(self: &Arc<Self>) -> StreamReader {
        self.lock().readers += 1;
        StreamReader {
            buffer: self.clone(),
            pos: 0,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panicking writer would otherwise poison the buffer and kill playback
        // for the rest of the session; the data itself stays consistent.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct StreamReader {
    buffer: Arc<StreamBuffer>,
    pos: u64,
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        // Readers are handed to players, which are replaced on track switches and
        // on the final rebuild onto the finished file. Without the decrement the
        // count only grows and `claim()` refuses every later attempt forever.
        let mut inner = self.buffer.lock();
        inner.readers = inner.readers.saturating_sub(1);
    }
}

/// One response's right to write into the mirror. Dropping it without `commit`
/// rolls the mirror back to empty, so a failed or cancelled attempt never leaves
/// bytes behind for the next one.
pub struct StreamClaim {
    buffer: Arc<StreamBuffer>,
    generation: u64,
    committed: bool,
}

impl StreamClaim {
    pub fn start(&self, total: Option<u64>, quality: &'static str, source: &'static str) {
        let mut inner = self.buffer.lock();
        if inner.generation != self.generation {
            return;
        }
        inner.total = total;
        inner.quality = Some(quality);
        inner.source = Some(source);
    }

    pub fn push(&self, chunk: &[u8]) {
        let mut inner = self.buffer.lock();
        if inner.generation != self.generation || !inner.tee || inner.finished || chunk.is_empty() {
            return;
        }
        inner.data.extend_from_slice(chunk);
        drop(inner);
        self.buffer.wake.notify_all();
    }

    pub fn commit(mut self) {
        self.committed = true;
        let mut inner = self.buffer.lock();
        if inner.generation == self.generation {
            inner.claimed = false;
            inner.sealed = true;
        }
    }
}

impl Drop for StreamClaim {
    fn drop(&mut self) {
        if !self.committed {
            self.buffer.abandon(self.generation);
        }
    }
}

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut inner = self.buffer.lock();
        loop {
            let pos = self.pos as usize;
            if pos < inner.data.len() {
                let n = (inner.data.len() - pos).min(buf.len());
                buf[..n].copy_from_slice(&inner.data[pos..pos + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if inner.finished {
                return Ok(0);
            }
            inner = match self.buffer.wake.wait_timeout(inner, WAIT_SLICE) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let inner = self.buffer.lock();
        let target = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => (self.pos as i64 + delta).max(0) as u64,
            SeekFrom::End(delta) => {
                let end = inner.total.unwrap_or(inner.data.len() as u64);
                (end as i64 + delta).max(0) as u64
            }
        };
        self.pos = target;
        Ok(target)
    }
}

/// Container layouts whose decoder can start on a partial file.
///
/// SoundCloud's progressive preset is MP3 (`mp3_1_0`) and its progressive AAC
/// arrives as bare ADTS or as ISO-BMFF; both decode from the first frames on.
/// An mp4 only qualifies when `moov` sits before `mdat` (ffmpeg `+faststart`, or
/// an HLS-fMP4 init segment). With `moov` at the tail the sample table is in the
/// last bytes of the file, so that case keeps the full-wait path.
pub fn early_start_container(head: &[u8]) -> Option<&'static str> {
    if head.len() < 12 {
        return None;
    }
    if head.starts_with(b"ID3") {
        return Some("mp3");
    }
    // MPEG audio and ADTS both start with an 11-bit frame sync.
    if head[0] == 0xff && (head[1] & 0xe0) == 0xe0 {
        return Some("mpeg");
    }
    if is_mp4_header(head) {
        return moov_before_mdat(head).then_some("mp4");
    }
    None
}

fn is_mp4_header(head: &[u8]) -> bool {
    head.len() >= 8 && &head[4..8] == b"ftyp"
}

/// Walk the top-level ISO-BMFF boxes in `head`. True only when a complete
/// `moov` box shows up before `mdat`; an incomplete box at the end of the sniff
/// window means the header did not fit, which also falls back to the file path.
fn moov_before_mdat(head: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset + 8 <= head.len() {
        let size32 = u32::from_be_bytes([
            head[offset],
            head[offset + 1],
            head[offset + 2],
            head[offset + 3],
        ]) as u64;
        let kind = &head[offset + 4..offset + 8];
        let size = if size32 == 1 {
            if offset + 16 > head.len() {
                return false;
            }
            u64::from_be_bytes([
                head[offset + 8],
                head[offset + 9],
                head[offset + 10],
                head[offset + 11],
                head[offset + 12],
                head[offset + 13],
                head[offset + 14],
                head[offset + 15],
            ])
        } else {
            size32
        };
        if size < 8 {
            // size 0 means "up to the end of the file"; either way no box follows.
            return false;
        }
        match kind {
            b"moov" => return offset.saturating_add(size as usize) <= head.len(),
            b"mdat" => return false,
            _ => {}
        }
        offset = match offset.checked_add(size as usize) {
            Some(next) => next,
            None => return false,
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{early_start_container, moov_before_mdat, StreamBuffer};

    /// A reader that is gone must not block the next response from taking the
    /// mirror: the count used to only grow, so one early start disabled the
    /// mirror for the rest of the session.
    #[test]
    fn dropping_the_reader_releases_the_mirror() {
        let buffer = StreamBuffer::new();
        let claim = buffer.claim().expect("first claim");
        claim.start(None, "sq", "anon");
        claim.push(b"abc");
        drop(claim); // failed attempt: abandon() clears the bytes

        let reader = buffer.reader();
        assert!(
            buffer.claim().is_none(),
            "a live decoder must block a retry from resetting the mirror"
        );
        drop(reader);
        assert!(
            buffer.claim().is_some(),
            "after the reader is gone the mirror is reusable"
        );
    }

    fn mp4(prefix: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for boxed in prefix {
            out.extend_from_slice(boxed);
        }
        out
    }

    fn box_of(kind: &[u8; 4], payload_len: usize) -> Vec<u8> {
        let size = (8 + payload_len) as u32;
        let mut out = Vec::from(size.to_be_bytes());
        out.extend_from_slice(kind);
        out.resize(size as usize, 0);
        out
    }

    #[test]
    fn front_moov_mp4_is_eligible() {
        let head = mp4(&[
            &box_of(b"ftyp", 16),
            &box_of(b"moov", 64),
            &box_of(b"mdat", 32),
        ]);
        assert!(moov_before_mdat(&head));
        assert_eq!(early_start_container(&head), Some("mp4"));
    }

    #[test]
    fn tail_moov_mp4_is_not_eligible() {
        let head = mp4(&[
            &box_of(b"ftyp", 16),
            &box_of(b"mdat", 4096),
            &box_of(b"moov", 64),
        ]);
        assert!(!moov_before_mdat(&head));
        assert_eq!(early_start_container(&head), None);
    }

    #[test]
    fn incomplete_moov_is_not_eligible() {
        let mut head = mp4(&[&box_of(b"ftyp", 16)]);
        // moov header only: the sample table did not fit the sniff window.
        head.extend_from_slice(&4000u32.to_be_bytes());
        head.extend_from_slice(b"moov");
        head.resize(head.len() + 16, 0);
        assert!(!moov_before_mdat(&head));
    }

    #[test]
    fn mp3_and_adts_are_eligible() {
        assert_eq!(early_start_container(b"ID3\x04\x00\x00\x00\x00\x00\x00\x00\x00"), Some("mp3"));
        assert_eq!(early_start_container(b"\xff\xfb\x90\x64\x00\x00\x00\x00\x00\x00\x00\x00"), Some("mpeg"));
        assert_eq!(early_start_container(b"\xff\xf1\x50\x80\x00\x00\x00\x00\x00\x00\x00\x00"), Some("mpeg"));
    }

    #[test]
    fn other_containers_keep_the_full_wait_path() {
        assert_eq!(early_start_container(b"OggS\x00\x02\x00\x00\x00\x00\x00\x00"), None);
        assert_eq!(early_start_container(b"fLaC\x00\x00\x00\x22\x00\x00\x00\x00"), None);
        assert_eq!(early_start_container(b"RIFF\x24\x00\x00\x00WAVEfmt "), None);
    }
}
