use std::io::Cursor;
use std::num::NonZero;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rodio::mixer::Mixer;
use rodio::source::SeekError;
use rodio::{Decoder, Player, Source};

use crate::audio::eq::{EqSource, GainSource};
use crate::audio::types::{
    ChannelCount, EqParams, SampleRate, NORMALIZATION_ANALYSIS_SAMPLES,
    NORMALIZATION_MAX_ATTENUATION_DB, NORMALIZATION_MAX_BOOST_DB, NORMALIZATION_TARGET_PEAK,
    NORMALIZATION_TARGET_RMS,
};

struct OpusSource {
    reader: ogg::reading::PacketReader<Cursor<Vec<u8>>>,
    decoder: audiopus::coder::Decoder,
    channels: ChannelCount,
    buffer: Vec<f32>,
    buf_pos: usize,
    serial: u32,
    pre_skip: usize,
    samples_skipped: usize,
}

impl OpusSource {
    fn new(data: Vec<u8>) -> Result<Self, String> {
        let mut reader = ogg::reading::PacketReader::new(Cursor::new(data));

        let head_pkt = reader
            .read_packet()
            .map_err(|e| format!("OGG read error: {}", e))?
            .ok_or("No OpusHead packet")?;

        let head = &head_pkt.data;
        if head.len() < 19 || &head[..8] != b"OpusHead" {
            return Err("Invalid OpusHead".into());
        }

        let serial = head_pkt.stream_serial();
        let ch_count = head[9];
        let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;
        let opus_ch = if ch_count == 1 {
            audiopus::Channels::Mono
        } else {
            audiopus::Channels::Stereo
        };

        reader
            .read_packet()
            .map_err(|e| format!("OGG read error: {}", e))?;

        let decoder = audiopus::coder::Decoder::new(audiopus::SampleRate::Hz48000, opus_ch)
            .map_err(|e| format!("Opus decoder error: {:?}", e))?;

        let channel_count = if ch_count == 1 { 1u16 } else { 2u16 };

        Ok(Self {
            reader,
            decoder,
            channels: NonZero::new(channel_count).unwrap(),
            buffer: Vec::new(),
            buf_pos: 0,
            serial,
            pre_skip: pre_skip * channel_count as usize,
            samples_skipped: 0,
        })
    }

    fn decode_next_packet(&mut self) -> bool {
        loop {
            match self.reader.read_packet() {
                Ok(Some(pkt)) => {
                    if pkt.data.is_empty() {
                        continue;
                    }
                    let channels = self.channels.get() as usize;
                    let mut buf = vec![0f32; 5760 * channels];
                    match self.decoder.decode_float(Some(&pkt.data), &mut buf, false) {
                        Ok(samples_per_ch) => {
                            let total = samples_per_ch * channels;
                            buf.truncate(total);

                            if self.samples_skipped < self.pre_skip {
                                let skip = (self.pre_skip - self.samples_skipped).min(total);
                                self.samples_skipped += skip;
                                if skip >= total {
                                    continue;
                                }
                                self.buffer = buf[skip..].to_vec();
                            } else {
                                self.buffer = buf;
                            }
                            self.buf_pos = 0;
                            return true;
                        }
                        Err(_) => continue,
                    }
                }
                _ => return false,
            }
        }
    }
}

impl Iterator for OpusSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.buf_pos >= self.buffer.len() && !self.decode_next_packet() {
            return None;
        }
        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl Source for OpusSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(48000).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let target_gp = (pos.as_secs_f64() * 48000.0) as u64;

        match self.reader.seek_absgp(Some(self.serial), target_gp) {
            Ok(_) => {
                let opus_ch = if self.channels.get() == 1 {
                    audiopus::Channels::Mono
                } else {
                    audiopus::Channels::Stereo
                };
                self.decoder =
                    audiopus::coder::Decoder::new(audiopus::SampleRate::Hz48000, opus_ch).map_err(
                        |_| SeekError::NotSupported {
                            underlying_source: "opus decoder reinit failed",
                        },
                    )?;
                self.buffer.clear();
                self.buf_pos = 0;
                self.samples_skipped = self.pre_skip;
                Ok(())
            }
            Err(_) => Err(SeekError::NotSupported {
                underlying_source: "ogg seek failed",
            }),
        }
    }
}

fn normalization_gain_from_samples<I>(samples: I) -> f32
where
    I: IntoIterator<Item = f32>,
{
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f64;
    let mut count = 0usize;

    for sample in samples.into_iter().take(NORMALIZATION_ANALYSIS_SAMPLES) {
        let value = sample as f64;
        let abs = value.abs();
        peak = peak.max(abs);
        sum_sq += value * value;
        count += 1;
    }

    if count == 0 {
        return 1.0;
    }

    let rms = (sum_sq / count as f64).sqrt().max(1e-6);
    let target_gain = NORMALIZATION_TARGET_RMS / rms;
    let peak_safe_gain = if peak > 0.0 {
        NORMALIZATION_TARGET_PEAK / peak
    } else {
        target_gain
    };

    let max_boost = 10f64.powf(NORMALIZATION_MAX_BOOST_DB / 20.0);
    let max_attenuation = 10f64.powf(NORMALIZATION_MAX_ATTENUATION_DB / 20.0);
    let gain = target_gain
        .min(peak_safe_gain)
        .clamp(max_attenuation, max_boost);

    if (gain - 1.0).abs() < 0.05 {
        1.0
    } else {
        gain as f32
    }
}

pub fn create_player_from_bytes(
    bytes: &[u8],
    mixer: &Mixer,
    volume: f32,
    normalization_enabled: bool,
    eq_params: Arc<RwLock<EqParams>>,
) -> Result<(Player, Option<f64>), String> {
    let player = Player::connect_new(mixer);
    player.set_volume(volume);

    let duration;
    if let Ok(source) = Decoder::new(Cursor::new(bytes.to_vec())) {
        let gain = if normalization_enabled {
            normalization_gain_from_samples(source)
        } else {
            1.0
        };
        let source = Decoder::new(Cursor::new(bytes.to_vec()))
            .map_err(|e| format!("Failed to decode: {}", e))?;
        duration = source.total_duration().map(|d| d.as_secs_f64());
        player.append(EqSource::new(GainSource::new(source, gain), eq_params));
    } else {
        let gain = if normalization_enabled {
            normalization_gain_from_samples(
                OpusSource::new(bytes.to_vec()).map_err(|e| format!("Failed to decode: {}", e))?,
            )
        } else {
            1.0
        };
        let source =
            OpusSource::new(bytes.to_vec()).map_err(|e| format!("Failed to decode: {}", e))?;
        duration = source.total_duration().map(|d| d.as_secs_f64());
        player.append(EqSource::new(GainSource::new(source, gain), eq_params));
    }

    Ok((player, duration))
}
