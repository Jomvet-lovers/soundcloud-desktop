use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Shared state between the download task and the audio thread.
struct Inner {
    bytes: Vec<u8>,
    complete: bool,
}

/// A growing byte buffer that implements `Read + Seek`.
///
/// The download task appends chunks via `push()` / `mark_complete()`.
/// The audio thread reads sequentially; if it catches up to the
/// download frontier it blocks (condvar) until more data arrives.
#[derive(Clone)]
pub struct StreamingBuffer {
    inner: Arc<(Mutex<Inner>, Condvar)>,
}

/// A reader handle with its own position cursor.
pub struct StreamingReader {
    shared: StreamingBuffer,
    pos: usize,
}

impl StreamingBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(Inner {
                    bytes: Vec::with_capacity(512 * 1024),
                    complete: false,
                }),
                Condvar::new(),
            )),
        }
    }

    /// Append a chunk. Called from the download task.
    pub fn push(&self, chunk: &[u8]) {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        state.bytes.extend_from_slice(chunk);
        cvar.notify_all();
    }

    /// Mark the download as finished. No more data will arrive.
    pub fn mark_complete(&self) {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        state.complete = true;
        cvar.notify_all();
    }

    /// Mark download as failed — unblocks waiting readers.
    pub fn mark_error(&self) {
        self.mark_complete();
    }

    /// Current buffered size.
    pub fn len(&self) -> usize {
        let (lock, _) = &*self.inner;
        lock.lock().unwrap().bytes.len()
    }

    /// Is download finished?
    pub fn is_complete(&self) -> bool {
        let (lock, _) = &*self.inner;
        lock.lock().unwrap().complete
    }

    /// Get a copy of all downloaded bytes (for caching to disk).
    pub fn snapshot(&self) -> Vec<u8> {
        let (lock, _) = &*self.inner;
        lock.lock().unwrap().bytes.clone()
    }

    /// Create a new reader positioned at the start.
    pub fn reader(&self) -> StreamingReader {
        StreamingReader {
            shared: self.clone(),
            pos: 0,
        }
    }
}

impl Read for StreamingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (lock, cvar) = &*self.shared.inner;

        loop {
            let state = lock.lock().unwrap();
            let available = state.bytes.len().saturating_sub(self.pos);

            if available > 0 {
                let n = buf.len().min(available);
                buf[..n].copy_from_slice(&state.bytes[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }

            if state.complete {
                return Ok(0); // EOF
            }

            // Wait for more data with timeout (prevents deadlock if download aborts)
            let (state, timeout) = cvar
                .wait_timeout(state, Duration::from_millis(200))
                .unwrap();

            if timeout.timed_out() && state.bytes.len() <= self.pos && !state.complete {
                // Still no data — keep waiting
                continue;
            }
        }
    }
}

impl Seek for StreamingReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let (lock, cvar) = &*self.shared.inner;

        let new_pos = match pos {
            SeekFrom::Start(n) => n as usize,
            SeekFrom::Current(n) => {
                if n >= 0 {
                    self.pos + n as usize
                } else {
                    self.pos.saturating_sub((-n) as usize)
                }
            }
            SeekFrom::End(n) => {
                // For SeekFrom::End, wait until download is complete
                let state = lock.lock().unwrap();
                if !state.complete {
                    drop(state);
                    // Wait for completion (with total timeout of 60s)
                    let mut state = lock.lock().unwrap();
                    let deadline = std::time::Instant::now() + Duration::from_secs(60);
                    while !state.complete {
                        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
                        if timeout.is_zero() {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "timed out waiting for download to complete for SeekFrom::End",
                            ));
                        }
                        let result = cvar.wait_timeout(state, timeout).unwrap();
                        state = result.0;
                    }
                    let total = state.bytes.len();
                    drop(state);
                    if n >= 0 {
                        total + n as usize
                    } else {
                        total.saturating_sub((-n) as usize)
                    }
                } else {
                    let total = state.bytes.len();
                    drop(state);
                    if n >= 0 {
                        total + n as usize
                    } else {
                        total.saturating_sub((-n) as usize)
                    }
                }
            }
        };

        // If seeking forward past what's downloaded, wait for data
        loop {
            let state = lock.lock().unwrap();
            if new_pos <= state.bytes.len() || state.complete {
                self.pos = new_pos.min(state.bytes.len());
                return Ok(self.pos as u64);
            }
            // Wait for more data
            let _ = cvar.wait_timeout(state, Duration::from_millis(200)).unwrap();
        }
    }
}
