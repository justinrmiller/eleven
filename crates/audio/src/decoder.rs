//! File and stream decoding via symphonia, producing interleaved stereo `f32` samples.

use std::path::Path;
use std::time::Duration;

use eleven_core::TrackInfo;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

use crate::error::{AudioError, Result};

/// How many undecodable packets in a row are tolerated before giving up.
const MAX_CONSECUTIVE_DECODE_ERRORS: usize = 64;

/// A demuxer + decoder pair for the default audio track of a source.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    sample_rate: u32,
    channels: usize,
    info: TrackInfo,
    scratch: Vec<f32>,
    /// After an accurate seek, frames before this time are discarded.
    skip_until: Option<f64>,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl Decoder {
    /// Opens a local file. The extension is used as a probing hint.
    ///
    /// # Errors
    /// Fails if the file cannot be read or its format/codec is unsupported.
    pub fn open_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|source| AudioError::Open {
            path: path.display().to_string(),
            source,
        })?;
        let byte_len = file.metadata().ok().map(|m| m.len());
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        let mut decoder = Self::open_source(Box::new(file), ext.as_deref(), None)?;
        // Average bitrate from the file size, as XMMS displays it.
        if let (Some(len), Some(d)) = (byte_len, decoder.info.duration)
            && d.as_secs_f64() > 0.0
        {
            // Bitrates are far below u32::MAX kbit/s; truncation is the intent.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let kbps = (len as f64 * 8.0 / d.as_secs_f64() / 1000.0).round() as u32;
            decoder.info.bitrate_kbps = Some(kbps);
        }
        Ok(decoder)
    }

    /// Opens any media source. `extension` and `mime_type` are optional probing hints.
    ///
    /// # Errors
    /// Fails if the format or codec is unsupported or there is no audio track.
    pub fn open_source(
        source: Box<dyn MediaSource>,
        extension: Option<&str>,
        mime_type: Option<&str>,
    ) -> Result<Self> {
        let seekable = source.is_seekable();
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }
        if let Some(mime) = mime_type {
            hint.mime_type(mime);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(AudioError::from_symphonia)?;
        let setup = TrackSetup::for_default_track(format.as_ref())?;
        let mut this = Self {
            format,
            decoder: setup.decoder,
            track_id: setup.track_id,
            time_base: setup.time_base,
            sample_rate: setup.sample_rate,
            channels: setup.channels,
            info: TrackInfo::default(),
            scratch: Vec::new(),
            skip_until: None,
        };
        this.info = TrackInfo {
            title: None,
            sample_rate: setup.sample_rate,
            // Channel counts are tiny; saturate rather than wrap just in case.
            channels: u16::try_from(setup.channels).unwrap_or(u16::MAX),
            bitrate_kbps: None,
            duration: setup.duration,
            seekable,
            codec: setup.codec,
        };
        Ok(this)
    }

    /// Rebuilds the decoder after the stream switched tracks (chained Ogg streams).
    fn reselect_track(&mut self) -> Result<()> {
        let setup = TrackSetup::for_default_track(self.format.as_ref())?;
        self.decoder = setup.decoder;
        self.track_id = setup.track_id;
        self.time_base = setup.time_base;
        self.sample_rate = setup.sample_rate;
        self.channels = setup.channels;
        self.info.sample_rate = setup.sample_rate;
        self.info.channels = u16::try_from(setup.channels).unwrap_or(u16::MAX);
        self.info.codec = setup.codec;
        Ok(())
    }

    /// Technical information about the track.
    #[must_use]
    pub fn info(&self) -> &TrackInfo {
        &self.info
    }

    /// Mutable access to the track information (e.g. to add a stream's bitrate).
    pub fn info_mut(&mut self) -> &mut TrackInfo {
        &mut self.info
    }

    /// Native sample rate of the track.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Decodes the next packet and appends interleaved stereo samples to `out`.
    ///
    /// Mono is duplicated to both channels; for more than two channels the first two are
    /// used. Returns `Ok(false)` at the end of the stream.
    ///
    /// # Errors
    /// Returns an error if too many packets in a row cannot be decoded or the source fails.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        let mut errors = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(SymError::ResetRequired) => {
                    // Chained streams (e.g. Ogg radio) switch tracks mid-stream.
                    self.reselect_track()?;
                    continue;
                }
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(AudioError::from_symphonia(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let packet_time = self.ts_to_secs(packet.pts);
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    buf.copy_to_vec_interleaved(&mut self.scratch);
                    let channels = buf.num_planes().max(1);
                    let frames = self.scratch.len() / channels;
                    let mut skip = 0;
                    if let (Some(target), Some(at)) = (self.skip_until, packet_time) {
                        let behind = (target - at) * f64::from(self.sample_rate);
                        if behind >= 1.0 {
                            // `behind` is positive and bounded by the packet length.
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let behind = behind as usize;
                            skip = behind.min(frames);
                        }
                    }
                    if skip < frames {
                        self.skip_until = None;
                    }
                    push_stereo(&self.scratch, channels, skip, out);
                    return Ok(true);
                }
                Err(SymError::DecodeError(_) | SymError::IoError(_)) => {
                    errors += 1;
                    if errors > MAX_CONSECUTIVE_DECODE_ERRORS {
                        return Err(AudioError::Decode("too many corrupt packets".into()));
                    }
                }
                Err(e) => return Err(AudioError::from_symphonia(e)),
            }
        }
    }

    /// Seeks to `position` (sample-accurately) and returns the position actually reached.
    ///
    /// # Errors
    /// Fails if the source is not seekable or the demuxer rejects the seek.
    pub fn seek(&mut self, position: Duration) -> Result<Duration> {
        if !self.info.seekable {
            return Err(AudioError::Seek("source is not seekable".into()));
        }
        let mut target = position;
        if let Some(d) = self.info.duration {
            target = target.min(d.saturating_sub(Duration::from_millis(1)));
        }
        let time = Time::try_from_secs_f64(target.as_secs_f64())
            .ok_or_else(|| AudioError::Seek("position out of range".into()))?;
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::Seek(e.to_string()))?;
        self.decoder.reset();
        self.skip_until = Some(target.as_secs_f64());
        Ok(target)
    }

    fn ts_to_secs(&self, ts: Timestamp) -> Option<f64> {
        let tb = self.time_base?;
        tb.calc_time(ts).map(|t| t.as_secs_f64())
    }
}

fn time_to_duration(time: Time) -> Option<Duration> {
    let secs = time.as_secs_f64();
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

/// Converts a frame count to a duration at `sample_rate`.
#[must_use]
pub fn frames_to_duration(frames: u64, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }
    let rate = u64::from(sample_rate);
    Duration::from_secs(frames / rate)
        + Duration::from_nanos((frames % rate) * 1_000_000_000 / rate)
}

/// Appends interleaved `channels`-channel audio as stereo, skipping the first `skip` frames.
pub fn push_stereo(interleaved: &[f32], channels: usize, skip: usize, out: &mut Vec<f32>) {
    let channels = channels.max(1);
    let frames = interleaved.chunks_exact(channels).skip(skip);
    out.reserve(frames.len() * 2);
    for frame in frames {
        let (l, r) = if channels == 1 {
            (frame[0], frame[0])
        } else {
            (frame[0], frame[1])
        };
        out.push(l);
        out.push(r);
    }
}

/// Everything needed to decode one track.
struct TrackSetup {
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    sample_rate: u32,
    channels: usize,
    duration: Option<Duration>,
    codec: String,
}

impl TrackSetup {
    fn for_default_track(format: &dyn FormatReader) -> Result<Self> {
        let track = format
            .default_track(TrackType::Audio)
            .or_else(|| format.first_track_known_codec(TrackType::Audio))
            .ok_or_else(|| AudioError::Unsupported("no audio track".into()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AudioError::Unsupported("track has no audio parameters".into()))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(AudioError::from_symphonia)?;
        let sample_rate = params.sample_rate.unwrap_or(44_100);
        let channels = params.channels.as_ref().map_or(2, |c| c.count().max(1));
        let duration = match (track.time_base, track.duration) {
            (Some(tb), Some(d)) => tb.calc_duration(d).and_then(time_to_duration),
            _ => track.num_frames.map(|n| frames_to_duration(n, sample_rate)),
        };
        let codec = decoder.codec_info().short_name.to_owned();
        Ok(Self {
            decoder,
            track_id: track.id,
            time_base: track.time_base,
            sample_rate,
            channels,
            duration,
            codec,
        })
    }
}
