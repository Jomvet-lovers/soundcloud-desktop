//! High-quality sample-rate conversion: windowed-sinc polyphase FIR.
//!
//! rodio's mixer converts mismatched rates with linear interpolation (up) /
//! sample dropping (down), which audibly dulls the high end (see rodio issue
//! #584). Every SoundCloud stream is 44.1 kHz while most Windows outputs open
//! at 48 kHz, so without this every track loses ~1–2 dB above 10 kHz against
//! the browser. We convert to the output rate before the mixer sees the source;
//! the mixer's own converter then takes its identity path.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

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

/// Fixed-point scale for the shared playback-rate atomic (`rate * 10000`).
pub const SPEED_FP_SCALE: u32 = 10_000;

const SPEED_HALF_TAPS: i64 = 12;
const SPEED_PHASES: usize = 512;

/// Playback-speed (and therefore pitch) control that actually resamples the
/// stream. rodio's own `Speed` only changes a source's reported sample rate,
/// and the mixer latches that rate at span boundaries — so mid-track speed
/// changes never reach the output. This source instead interpolates with a
/// windowed-sinc phase bank, reads the shared factor every frame (immediate
/// response) and stays bit-exact at 1x.
pub struct SpeedSource<S: Source<Item = f32>> {
    input: S,
    factor_fp: Arc<AtomicU32>,
    channels: usize,
    /// `SPEED_PHASES` × `2 * SPEED_HALF_TAPS` taps for the current cutoff.
    coeffs: Vec<f32>,
    coeff_cutoff: f64,
    history: VecDeque<f32>,
    next_index: i64,
    last_index: i64,
    eof: bool,
    /// Input frame position of the next output frame.
    pos: f64,
    frame_buf: Vec<f32>,
    frame_pos: usize,
}

impl<S: Source<Item = f32>> SpeedSource<S> {
    pub fn new(input: S, factor_fp: Arc<AtomicU32>) -> Self {
        let channels = input.channels().get() as usize;
        let cutoff = ROLLOFF;
        Self {
            input,
            factor_fp,
            channels,
            coeffs: build_speed_coeffs(cutoff),
            coeff_cutoff: cutoff,
            history: VecDeque::new(),
            next_index: 0,
            last_index: -1,
            eof: false,
            pos: 0.0,
            frame_buf: vec![0.0; channels],
            frame_pos: channels,
        }
    }

    fn factor(&self) -> f32 {
        (self.factor_fp.load(Ordering::Relaxed) as f32 / SPEED_FP_SCALE as f32).clamp(0.25, 4.0)
    }

    fn frames_in_history(&self) -> i64 {
        self.history.len() as i64 / self.channels.max(1) as i64
    }

    fn pull_until(&mut self, want_index: i64) {
        while !self.eof && self.next_index <= want_index {
            for _ in 0..self.channels {
                match self.input.next() {
                    Some(sample) => self.history.push_back(sample),
                    None => {
                        self.eof = true;
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

    fn sample_at(&self, idx: i64, channel: usize) -> f32 {
        let frames = self.frames_in_history();
        let offset = idx - (self.next_index - frames);
        if offset < 0 || offset >= frames {
            return 0.0;
        }
        self.history[offset as usize * self.channels + channel]
    }

    fn drop_before(&mut self, min_index: i64) {
        let mut oldest = self.next_index - self.frames_in_history();
        while oldest < min_index && self.frames_in_history() > 0 {
            for _ in 0..self.channels {
                self.history.pop_front();
            }
            oldest += 1;
        }
    }

    /// Rebuild the interpolation bank when the anti-alias cutoff changes
    /// (speeding up must band-limit the source to avoid aliasing).
    fn ensure_coeffs(&mut self, factor: f32) {
        let cutoff = (ROLLOFF * (1.0 / factor as f64).min(1.0)).clamp(0.3, ROLLOFF);
        let cutoff = (cutoff * 64.0).round() / 64.0;
        if (cutoff - self.coeff_cutoff).abs() > 1e-9 {
            self.coeffs = build_speed_coeffs(cutoff);
            self.coeff_cutoff = cutoff;
        }
    }

    fn compute_frame(&mut self) -> bool {
        let factor = self.factor();
        let exact = (factor - 1.0).abs() < 1e-6;
        let base = self.pos.floor();
        let taps = (2 * SPEED_HALF_TAPS) as usize;
        let (phase, first, last_needed) = if exact {
            (0usize, base as i64, base as i64)
        } else {
            self.ensure_coeffs(factor);
            let frac = self.pos - base;
            let phase = ((frac * SPEED_PHASES as f64) as usize).min(SPEED_PHASES - 1);
            let first = base as i64 - (SPEED_HALF_TAPS - 1);
            (phase, first, base as i64 + SPEED_HALF_TAPS)
        };

        self.pull_until(last_needed);
        if self.eof && first > self.last_index {
            return false;
        }
        self.drop_before(first);

        if exact {
            for channel in 0..self.channels {
                self.frame_buf[channel] = self.sample_at(base as i64, channel);
            }
        } else {
            let coeff = &self.coeffs[phase * taps..phase * taps + taps];
            for channel in 0..self.channels {
                let mut sum = 0.0f32;
                let mut idx = first;
                for &k in coeff {
                    sum += self.sample_at(idx, channel) * k;
                    idx += 1;
                }
                self.frame_buf[channel] = sum;
            }
        }

        self.pos += factor as f64;
        self.frame_pos = 0;
        true
    }
}

impl<S: Source<Item = f32>> Iterator for SpeedSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
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

impl<S: Source<Item = f32>> Source for SpeedSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input
            .total_duration()
            .map(|d| d.div_f32(self.factor()))
    }

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), SeekError> {
        // `pos` arrives in output (wall-clock) time, same contract as rodio's
        // own `Speed`: the decoder must move `factor` times further.
        self.input.try_seek(pos.mul_f32(self.factor()))?;
        self.history.clear();
        self.next_index = 0;
        self.last_index = -1;
        self.eof = false;
        self.pos = 0.0;
        self.frame_pos = self.channels;
        Ok(())
    }
}

fn build_speed_coeffs(cutoff: f64) -> Vec<f32> {
    let taps = (2 * SPEED_HALF_TAPS) as usize;
    let mut coeffs = vec![0f32; SPEED_PHASES * taps];
    let half = SPEED_HALF_TAPS as f64;

    for phase in 0..SPEED_PHASES {
        let frac = phase as f64 / SPEED_PHASES as f64;
        let mut sum = 0.0f64;
        for k in 0..taps {
            let j = k as i64 - (SPEED_HALF_TAPS - 1);
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

    /// Play `source` through a rodio player + 48k mixer and return the number of
    /// output frames that carry signal (i.e. the audible duration in frames).
    fn audible_frames(source: impl rodio::Source<Item = f32> + Send + 'static) -> u64 {
        use rodio::Player;
        use std::num::NonZero;

        let (mixer, mut out) = rodio::mixer::mixer(
            NonZero::new(2).unwrap(),
            NonZero::new(48000).unwrap(),
        );
        let player = Player::connect_new(&mixer);
        player.append(source);

        let mut frame: u64 = 0;
        let mut last_signal: u64 = 0;
        let mut silent_run: u64 = 0;
        loop {
            let Some(left) = out.next() else { break };
            let right = out.next().unwrap_or(0.0);
            if left.abs() > 1e-4 || right.abs() > 1e-4 {
                last_signal = frame;
                silent_run = 0;
            } else if frame > 0 {
                silent_run += 1;
                if silent_run > 4800 {
                    break;
                }
            }
            frame += 1;
            if frame > 48000 * 8 {
                break;
            }
        }
        last_signal
    }

    fn stereo_tone(freq: f64, rate: u32, secs: f64) -> SamplesBuffer {
        let data = tone(freq, rate, secs, 0.5);
        let mut interleaved = Vec::with_capacity(data.len() * 2);
        for s in data {
            interleaved.push(s);
            interleaved.push(s);
        }
        SamplesBuffer::new(
            std::num::NonZero::new(2).unwrap(),
            std::num::NonZero::new(rate).unwrap(),
            interleaved,
        )
    }

    #[test]
    fn player_plays_one_second_in_one_second() {
        let source = ResampleSource::new(stereo_tone(440.0, 44100, 1.0), 48000);
        let frames = audible_frames(source);
        assert!(
            frames.abs_diff(48000) < 2400,
            "expected ~48000 frames, got {frames}"
        );
    }

    fn mono_tone(freq: f64, rate: u32, secs: f64) -> SamplesBuffer {
        SamplesBuffer::new(
            std::num::NonZero::new(1).unwrap(),
            std::num::NonZero::new(rate).unwrap(),
            tone(freq, rate, secs, 0.5),
        )
    }

    #[test]
    fn speed_source_is_bit_exact_at_1x() {
        let input = tone(440.0, 44100, 0.1, 0.5);
        let expected = input.clone();
        let fp = Arc::new(AtomicU32::new(SPEED_FP_SCALE));
        let output: Vec<f32> = SpeedSource::new(mono_tone(440.0, 44100, 0.1), fp).collect();
        assert_eq!(expected, output);
    }

    #[test]
    fn speed_source_plays_faster() {
        let fp = Arc::new(AtomicU32::new(SPEED_FP_SCALE * 2));
        let output: Vec<f32> =
            SpeedSource::new(mono_tone(440.0, 44100, 2.0), fp).collect();
        // 2 s at 2x → ~1 s of frames at the source rate.
        assert!(
            output.len().abs_diff(44100) < 1000,
            "expected ~44100 frames, got {}",
            output.len()
        );
    }

    #[test]
    fn speed_source_applies_mid_stream() {
        // The factor is read per output frame, so changes must take effect
        // immediately (rodio's mixer never re-reads the rate mid-span).
        let fp = Arc::new(AtomicU32::new(SPEED_FP_SCALE));
        let mut source = SpeedSource::new(mono_tone(440.0, 44100, 2.0), fp.clone());
        let mut frames = 0usize;
        for _ in 0..44100 {
            assert!(source.next().is_some());
            frames += 1;
        }
        fp.store(SPEED_FP_SCALE * 2, Ordering::Relaxed);
        frames += source.count();
        // 1 s at 1x + 1 s at 2x ≈ 66150 frames.
        assert!(frames.abs_diff(66150) < 2000, "frames {frames}");
    }
}

