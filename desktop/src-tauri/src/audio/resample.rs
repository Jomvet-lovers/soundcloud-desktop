//! High-quality sample-rate conversion: windowed-sinc polyphase FIR.
//!
//! rodio's mixer converts mismatched rates with linear interpolation (up) /
//! sample dropping (down), which audibly dulls the high end (see rodio issue
//! #584). Every SoundCloud stream is 44.1 kHz while most Windows outputs open
//! at 48 kHz, so without this every track loses ~1–2 dB above 10 kHz against
//! the browser. We convert to the output rate before the mixer sees the source;
//! the mixer's own converter then takes its identity path.

use std::collections::VecDeque;

use rodio::source::SeekError;
use rodio::Source;

use crate::audio::types::{ChannelCount, SampleRate};

/// Windowed-sinc half-length in input samples (96 taps per phase).
const HALF_TAPS: i64 = 48;
/// Keep a little margin below the Nyquist frequency.
const ROLLOFF: f64 = 0.94;

pub struct ResampleSource<S: Source<Item = f32>> {
    input: S,
    channels: usize,
    out_rate: u32,
    passthrough: bool,
    /// Upsampling factor of the reduced ratio.
    up: u64,
    /// Downsampling factor of the reduced ratio.
    down: u64,
    /// `up` phases × `2 * HALF_TAPS` taps, DC-normalized per phase.
    coeffs: Vec<f32>,
    /// Input frames (interleaved) starting at `next_index - frames`.
    history: VecDeque<f32>,
    /// Absolute index of the next input frame to pull.
    next_index: i64,
    /// Absolute index of the last input frame pulled, or -1 before the first.
    last_index: i64,
    /// Absolute index of the next output frame.
    out_index: u64,
    eof: bool,
    /// Interleaved samples of the output frame currently being yielded.
    frame_buf: Vec<f32>,
    frame_pos: usize,
}

impl<S: Source<Item = f32>> ResampleSource<S> {
    pub fn new(input: S, target_rate: u32) -> Self {
        let in_rate = input.sample_rate().get();
        let out_rate = if target_rate == 0 { in_rate } else { target_rate };
        let channels = input.channels().get() as usize;
        let passthrough = in_rate == out_rate || channels == 0;
        let (up, down, coeffs) = if passthrough {
            (1, 1, Vec::new())
        } else {
            let g = gcd(in_rate as u64, out_rate as u64);
            let up = out_rate as u64 / g;
            let down = in_rate as u64 / g;
            (up, down, build_coeffs(up, down))
        };

        Self {
            input,
            channels,
            out_rate,
            passthrough,
            up,
            down,
            coeffs,
            history: VecDeque::new(),
            next_index: 0,
            last_index: -1,
            out_index: 0,
            eof: false,
            frame_buf: vec![0.0; channels],
            frame_pos: channels,
        }
    }

    fn frames_in_history(&self) -> i64 {
        self.history.len() as i64 / self.channels.max(1) as i64
    }

    /// Pull input frames until `want_index` is available (or the input ends).
    fn pull_until(&mut self, want_index: i64) {
        while !self.eof && self.next_index <= want_index {
            for _ in 0..self.channels {
                match self.input.next() {
                    Some(sample) => self.history.push_back(sample),
                    None => {
                        self.eof = true;
                        // Drop the partially read frame, if any.
                        while self.history.len() % self.channels != 0 {
                            self.history.pop_back();
                        }
                        return;
                    }
                }
            }
            self.last_index = self.next_index;
            self.next_index += 1;
        }
    }

    /// One input sample at the absolute frame index `idx`; missing (pre-roll or
    /// dropped) frames read as silence.
    fn sample_at(&self, idx: i64, channel: usize) -> f32 {
        let frames = self.frames_in_history();
        let offset = idx - (self.next_index - frames);
        if offset < 0 || offset >= frames {
            return 0.0;
        }
        self.history[offset as usize * self.channels + channel]
    }

    fn drop_before(&mut self, min_index: i64) {
        let frames = self.frames_in_history();
        let mut oldest = self.next_index - frames;
        while oldest < min_index && self.frames_in_history() > 0 {
            for _ in 0..self.channels {
                self.history.pop_front();
            }
            oldest += 1;
        }
    }

    /// Compute the next output frame into `frame_buf`. Returns false at EOF.
    fn compute_frame(&mut self) -> bool {
        loop {
            let num = self.out_index as u128 * self.down as u128;
            let base = (num / self.up as u128) as i64;
            let phase = (num % self.up as u128) as usize;

            let want = base + HALF_TAPS;
            self.pull_until(want);

            let first = base - HALF_TAPS + 1;
            if self.eof && first > self.last_index {
                return false;
            }
            self.drop_before(first);

            let taps = (2 * HALF_TAPS) as usize;
            let coeff = &self.coeffs[phase * taps..phase * taps + taps];
            for channel in 0..self.channels {
                let mut sum = 0.0f32;
                let mut idx = first;
                for &c in coeff {
                    sum += self.sample_at(idx, channel) * c;
                    idx += 1;
                }
                self.frame_buf[channel] = sum;
            }
            self.out_index += 1;
            self.frame_pos = 0;
            return true;
        }
    }
}

impl<S: Source<Item = f32>> Iterator for ResampleSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.passthrough {
            return self.input.next();
        }
        if self.frame_pos >= self.channels {
            if !self.compute_frame() {
                return None;
            }
        }
        let sample = self.frame_buf[self.frame_pos];
        self.frame_pos += 1;
        Some(sample)
    }
}

impl<S: Source<Item = f32>> Source for ResampleSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.out_rate).expect("output sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)?;
        self.history.clear();
        self.next_index = 0;
        self.last_index = -1;
        self.out_index = 0;
        self.eof = false;
        self.frame_pos = self.channels;
        Ok(())
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Blackman–Harris window over `t` in `(-1, 1)` (0 outside).
fn blackman_harris(t: f64) -> f64 {
    const A0: f64 = 0.35875;
    const A1: f64 = 0.48829;
    const A2: f64 = 0.14128;
    const A3: f64 = 0.01168;
    let pi = std::f64::consts::PI;
    A0 + A1 * (pi * t).cos() + A2 * (2.0 * pi * t).cos() + A3 * (3.0 * pi * t).cos()
}

/// Precompute one coefficient bank per phase for the reduced ratio `up/down`.
fn build_coeffs(up: u64, down: u64) -> Vec<f32> {
    let taps = (2 * HALF_TAPS) as usize;
    let mut coeffs = vec![0f32; up as usize * taps];
    let ratio = up as f64 / down as f64;
    let cutoff = ROLLOFF * ratio.min(1.0);
    let half = HALF_TAPS as f64;

    for phase in 0..up as usize {
        let frac = phase as f64 / up as f64;
        let mut sum = 0.0f64;
        for k in 0..taps {
            let j = k as i64 - (HALF_TAPS - 1);
            let u = frac - j as f64;
            let h = if u.abs() >= half {
                0.0
            } else {
                let x = std::f64::consts::PI * cutoff * u;
                let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
                sinc * blackman_harris(u / half)
            };
            coeffs[phase * taps + k] = h as f32;
            sum += h;
        }
        if sum != 0.0 {
            for k in 0..taps {
                coeffs[phase * taps + k] = (coeffs[phase * taps + k] as f64 / sum) as f32;
            }
        }
    }

    coeffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn tone(freq: f64, rate: u32, secs: f64, amp: f32) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (amp as f64 * (2.0 * std::f64::consts::PI * freq * t).sin()) as f32
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    fn run(input: Vec<f32>, from: u32, to: u32) -> Vec<f32> {
        let source = SamplesBuffer::new(
            std::num::NonZero::new(1).unwrap(),
            std::num::NonZero::new(from).unwrap(),
            input,
        );
        ResampleSource::new(source, to).collect()
    }

    fn gain_db(reference: &[f32], output: &[f32]) -> f64 {
        let skip = 4096;
        let a = rms(&reference[skip..]);
        let b = rms(&output[skip..output.len() - skip]);
        20.0 * (b / a).log10()
    }

    #[test]
    fn upsample_keeps_the_passband_flat() {
        // Linear interpolation (rodio's mixer) loses ~2 dB at 15 kHz on
        // 44.1k -> 48k; a proper resampler must stay within a fraction of a dB.
        for freq in [1000.0, 10000.0, 15000.0] {
            let input = tone(freq, 44100, 0.5, 0.5);
            let output = run(input.clone(), 44100, 48000);
            let db = gain_db(&input, &output);
            assert!(db.abs() < 0.4, "{freq} Hz: {db:.2} dB");
        }
    }

    #[test]
    fn downsample_rejects_the_alias() {
        // 23 kHz has no representation below the 44.1k Nyquist; dropping
        // samples would fold it back into the audible band.
        let input = tone(23000.0, 48000, 0.5, 0.5);
        let output = run(input.clone(), 48000, 44100);
        let db = gain_db(&input, &output);
        assert!(db < -40.0, "alias leaked at {db:.2} dB");
    }

    #[test]
    fn identity_when_rates_match() {
        let input = tone(440.0, 48000, 0.05, 0.5);
        let output = run(input.clone(), 48000, 48000);
        assert_eq!(input, output);
    }
}

