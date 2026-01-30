use std::path::Path;
use std::fs::File;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::audio::{SampleBuffer, AudioBufferRef};

use crate::alsa::device::BitPerfectDevice;
use crate::gui::PlayerState;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("ALSA error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Symphonia error: {0}")]
    Symphonia(#[from] symphonia::core::errors::Error),
    #[error("No audio track found")]
    NoAudioTrack,
}

pub struct BitPerfectPlayer {
    device: BitPerfectDevice,
}

impl BitPerfectPlayer {
    pub fn new(device: BitPerfectDevice) -> Self {
        Self { device }
    }

    pub fn play_file(&mut self, path: &Path, state: Arc<Mutex<PlayerState>>) -> Result<(), PlayerError> {
        let src_file = File::open(path).map_err(|e| {
            if let Ok(mut s) = state.lock() {
                s.error_message = Some(format!("File not found: {}", path.display()));
            }
            e
        })?;
        let mss = MediaSourceStream::new(Box::new(src_file), Default::default());

        let hint = Hint::new();
        let meta_opts: MetadataOptions = Default::default();
        let fmt_opts: FormatOptions = Default::default();
        
        let mut probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| {
                if let Ok(mut s) = state.lock() {
                    s.error_message = Some(format!("Decoding error: {}", e));
                }
                e
            })?;

        let mut format = probed.format;
        
        // Find the first audio track and copy its parameters to avoid borrowing 'format'
        let track_params = format.tracks().iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .map(|t| t.codec_params.clone())
            .ok_or_else(|| {
                if let Ok(mut s) = state.lock() {
                    s.error_message = Some("No valid audio track found".into());
                }
                PlayerError::NoAudioTrack
            })?;

        let dec_opts: DecoderOptions = Default::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track_params, &dec_opts)
            .map_err(|e| {
                if let Ok(mut s) = state.lock() {
                    s.error_message = Some(format!("Codec error: {}", e));
                }
                e
            })?;

        let sample_rate = track_params.sample_rate.ok_or(PlayerError::NoAudioTrack)?;
        let channels = track_params.channels.ok_or(PlayerError::NoAudioTrack)?.count() as u8;
        let bit_depth = track_params.bits_per_sample.unwrap_or(16) as u16;

        self.device.configure_exact(sample_rate, bit_depth, channels).map_err(|e| {
            if let Ok(mut s) = state.lock() {
                s.error_message = Some(format!("ALSA hardware error: {}", e));
            }
            e
        })?;

        // Update state with actual info
        {
            let mut s = state.lock().unwrap();
            
            // Extract metadata if available
            let mut title = None;
            let mut artist = None;
            let mut album_art = None;

            // 1. Check metadata in the format reader
            if let Some(meta) = format.metadata().current() {
                for tag in meta.tags() {
                    if let Some(std_key) = tag.std_key {
                        match std_key {
                            symphonia::core::meta::StandardTagKey::TrackTitle => title = Some(tag.value.to_string()),
                            symphonia::core::meta::StandardTagKey::Artist => artist = Some(tag.value.to_string()),
                            _ => {}
                        }
                    }
                }
                if let Some(visual) = meta.visuals().first() {
                    album_art = Some(visual.data.to_vec());
                }
            }
            
            // 2. If still missing info, check the probe metadata
            if title.is_none() || album_art.is_none() {
                if let Some(meta) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
                    for tag in meta.tags() {
                        if let Some(std_key) = tag.std_key {
                            match std_key {
                                symphonia::core::meta::StandardTagKey::TrackTitle => {
                                    if title.is_none() { title = Some(tag.value.to_string()); }
                                }
                                symphonia::core::meta::StandardTagKey::Artist => {
                                    if artist.is_none() { artist = Some(tag.value.to_string()); }
                                }
                                _ => {}
                            }
                        }
                    }
                    if album_art.is_none() {
                        if let Some(visual) = meta.visuals().first() {
                            album_art = Some(visual.data.to_vec());
                        }
                    }
                }
            }

            if let Some(ref mut track_info) = s.current_track {
                track_info.sample_rate = sample_rate;
                track_info.bit_depth = bit_depth;
                track_info.title = title;
                track_info.artist = artist;
            }
            s.album_art = album_art;
            
            s.error_message = None;
            s.duration_secs = track_params.n_frames
                .map(|f| f as f64 / sample_rate as f64)
                .unwrap_or(0.0);
            s.position_secs = 0.0;
        }

        loop {
            // Check if we should stop or if we are paused
            {
                let s = state.lock().unwrap();
                
                // Break if a command (Next/Prev/PlayIndex) is pending
                if s.command.is_some() {
                    break;
                }

                if !s.is_playing {
                    // Pause or stop
                    drop(s);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
            }

            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(_)) => break,
                Err(err) => return Err(PlayerError::Symphonia(err)),
            };

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let volume = {
                        state.lock().unwrap().volume_db
                    };
                    self.write_decoded_to_device(decoded, bit_depth, volume)?;
                    
                    let mut s = state.lock().unwrap();
                    s.position_secs += packet.dur() as f64 / sample_rate as f64;
                }
                Err(symphonia::core::errors::Error::DecodeError(err)) => {
                    eprintln!("Decode error: {}", err);
                }
                Err(err) => return Err(PlayerError::Symphonia(err)),
            }
        }

        self.device.drain()?;
        Ok(())
    }

    fn write_decoded_to_device(&mut self, decoded: AudioBufferRef, bit_depth: u16, volume_db: f64) -> Result<(), PlayerError> {
        let multiplier = 10.0f64.powf(volume_db / 20.0);

        match bit_depth {
            16 => {
                let mut sample_buf = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
                sample_buf.copy_interleaved_ref(decoded);
                let samples = sample_buf.samples_mut();
                
                // Apply volume
                if volume_db < 0.0 {
                    for s in samples.iter_mut() {
                        *s = (*s as f64 * multiplier) as i16;
                    }
                }

                let bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2)
                };
                self.device.write_raw(bytes)?;
            }
            24 | 32 => {
                let mut sample_buf = SampleBuffer::<i32>::new(decoded.capacity() as u64, *decoded.spec());
                sample_buf.copy_interleaved_ref(decoded);
                let samples = sample_buf.samples_mut();

                // Apply volume
                if volume_db < 0.0 {
                    for s in samples.iter_mut() {
                        *s = (*s as f64 * multiplier) as i32;
                    }
                }

                let bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 4)
                };
                self.device.write_raw(bytes)?;
            }
            _ => eprintln!("Unsupported bit depth: {}", bit_depth),
        }
        Ok(())
    }
}
