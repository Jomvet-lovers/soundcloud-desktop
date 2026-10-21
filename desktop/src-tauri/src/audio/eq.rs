use std::sync::{Arc, RwLock};
use std::time::Duration;

use biquad::{Biquad, Coefficients, DirectForm1, Hertz, ToHertz, Type, Q_BUTTERWORTH_F64};
use rodio::source::SeekError;
use rodio::Source;

use crate::audio::types::{ChannelCount, EqParams, SampleRate, EQ_BANDS, EQ_FREQS, EQ_Q};

/// Pass-through coefficients for a band that cannot exist at this output rate.
/// The top band sits at 16 kHz, which biquad rejects on an 8 kHz sink (Bluetooth
/// HFP/CVSD, some virtual cables). Using identity keeps the band silent instead
/// of panicking the audio thread on such a device.
fn pass_through() -> Coefficients<f64> {
    Coefficients {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    }
}

pub struct GainSource<S: Source<Item = f32>> {
    source: S,
    gain: f32,
}

impl<S: Source<Item = f32>> GainSource<S> {
    pub fn new(source: S, gain: f32) -> Self {
        Self { source, gain }
    }
}

impl<S: Source<Item = f32>> Iterator for GainSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // No hard clamp here: clipping before the player's volume stage bakes
        // distortion into the signal even at low listening levels. Overshoots
        // stay float and scale down with volume like in any other player.
        self.source.next().map(|sample| sample * self.gain)
    }
}

impl<S: Source<Item = f32>> Source for GainSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.source.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.source.try_seek(pos)
    }
}

pub struct EqSource<S: Source<Item = f32>> {
    source: S,
    params: Arc<RwLock<EqParams>>,
    filters_l: [DirectForm1<f64>; EQ_BANDS],
    filters_r: [DirectForm1<f64>; EQ_BANDS],
    channels: ChannelCount,
    sample_rate: SampleRate,
    current_channel: u16,
    cached_gains: [f64; EQ_BANDS],
    cached_enabled: bool,
    /// Samples left before the shared params are polled again (see `next`).
    poll_countdown: u32,
}

/// How often the audio thread re-reads the EQ params. 48 kHz stereo meant 96k
/// lock acquisitions per second for a value that only changes when the user
/// drags a band; 64 samples is ~1.3 ms of latency, well below perception.
const PARAM_POLL_STRIDE: u32 = 64;

impl<S: Source<Item = f32>> EqSource<S> {
    pub fn new(source: S, params: Arc<RwLock<EqParams>>) -> Self {
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        let fs: Hertz<f64> = (sample_rate.get() as f64).hz();

        let make_filters = || {
            std::array::from_fn(|i| {
                let filter_type = if i == 0 {
                    Type::LowShelf(0.0)
                } else if i == EQ_BANDS - 1 {
                    Type::HighShelf(0.0)
                } else {
                    Type::PeakingEQ(0.0)
                };
                let q = if i == 0 || i == EQ_BANDS - 1 {
                    Q_BUTTERWORTH_F64
                } else {
                    EQ_Q
                };
                let coeffs =
                    Coefficients::<f64>::from_params(filter_type, fs, EQ_FREQS[i].hz(), q)
                        .unwrap_or_else(|_| pass_through());
                DirectForm1::<f64>::new(coeffs)
            })
        };

        Self {
            source,
            params,
            filters_l: make_filters(),
            filters_r: make_filters(),
            channels,
            sample_rate,
            current_channel: 0,
            cached_gains: [0.0; EQ_BANDS],
            cached_enabled: false,
            poll_countdown: 0,
        }
    }

    fn update_coefficients(&mut self, gains: &[f64; EQ_BANDS]) {
        let fs: Hertz<f64> = (self.sample_rate.get() as f64).hz();
        for i in 0..EQ_BANDS {
            if (gains[i] - self.cached_gains[i]).abs() < 0.01 {
                continue;
            }

            let filter_type = if i == 0 {
                Type::LowShelf(gains[i])
            } else if i == EQ_BANDS - 1 {
                Type::HighShelf(gains[i])
            } else {
                Type::PeakingEQ(gains[i])
            };
            let q = if i == 0 || i == EQ_BANDS - 1 {
                Q_BUTTERWORTH_F64
            } else {
                EQ_Q
            };

            if let Ok(coeffs) =
                Coefficients::<f64>::from_params(filter_type, fs, EQ_FREQS[i].hz(), q)
            {
                self.filters_l[i] = DirectForm1::<f64>::new(coeffs);
                self.filters_r[i] = DirectForm1::<f64>::new(coeffs);
            }
        }

        self.cached_gains = *gains;
    }
}

impl<S: Source<Item = f32>> Iterator for EqSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.source.next()?;
        let ch = self.current_channel;
        self.current_channel = (ch + 1) % self.channels.get();

        // Poll the shared params on a stride, not on every sample: the filters
        // themselves still run per sample, only the lock read is sampled.
        if self.poll_countdown == 0 {
            self.poll_countdown = PARAM_POLL_STRIDE;
            let snapshot = self.params.try_read().ok().map(|p| (p.enabled, p.gains));
            if let Some((enabled, gains)) = snapshot
                && (enabled != self.cached_enabled || gains != self.cached_gains) {
                    if enabled {
                        self.update_coefficients(&gains);
                    }
                    self.cached_enabled = enabled;
                }
        } else {
            self.poll_countdown -= 1;
        }

        if !self.cached_enabled {
            return Some(sample);
        }

        let mut out = sample as f64;
        let filters = if ch == 0 {
            &mut self.filters_l
        } else {
            &mut self.filters_r
        };
        for filter in filters.iter_mut() {
            out = Biquad::run(filter, out);
        }

        // Left unclamped on purpose: see GainSource — clipping must happen
        // after the volume stage (or not at all).
        Some(out as f32)
    }
}

impl<S: Source<Item = f32>> Source for EqSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.source.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::{pass_through, EqSource, PARAM_POLL_STRIDE};
    use crate::audio::types::EqParams;
    use biquad::Biquad;
    use rodio::buffer::SamplesBuffer;
    use rodio::Source;
    use std::sync::{Arc, RwLock};

    fn params() -> Arc<RwLock<EqParams>> {
        Arc::new(RwLock::new(EqParams::default()))
    }

    fn source(rate: u32) -> SamplesBuffer {
        SamplesBuffer::new(
            std::num::NonZero::new(2).expect("channels"),
            std::num::NonZero::new(rate).expect("sample rate"),
            vec![0.0f32; 512],
        )
    }

    /// An 8 kHz sink (Bluetooth HFP, some virtual cables) cannot host the 16 kHz
    /// band; building the chain used to unwrap `OutsideNyquist` and panic.
    #[test]
    fn builds_on_a_sink_below_the_top_band() {
        let mut eq = EqSource::new(source(8_000), params());
        assert_eq!(eq.sample_rate().get(), 8_000);
        assert!(eq.next().is_some());
    }

    /// The fallback coefficients are a pass-through, not silence.
    #[test]
    fn missing_band_falls_back_to_identity() {
        let mut filter = biquad::DirectForm1::<f64>::new(pass_through());
        let out = Biquad::run(&mut filter, 0.25);
        assert!((out - 0.25).abs() < 1e-12);
    }

    /// The shared params are polled on a stride, not on every sample.
    #[test]
    fn params_are_polled_on_a_stride() {
        let mut eq = EqSource::new(source(48_000), params());
        // First sample polls (countdown starts at 0) and arms the stride.
        assert_eq!(eq.poll_countdown, 0);
        eq.next();
        assert_eq!(eq.poll_countdown, PARAM_POLL_STRIDE);
        // The next STRIDE samples only decrement; the one after polls again.
        for _ in 0..PARAM_POLL_STRIDE {
            eq.next();
        }
        assert_eq!(eq.poll_countdown, 0);
        eq.next();
        assert_eq!(eq.poll_countdown, PARAM_POLL_STRIDE);
    }
}
