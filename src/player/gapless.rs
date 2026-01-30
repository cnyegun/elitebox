use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::gui::PlayerState;
use crate::player::bitperfect::{BitPerfectPlayer, PlayerError};
use crate::alsa::device::BitPerfectDevice;

#[derive(Debug, Error)]
pub enum GaplessError {
    #[error("Player error: {0}")]
    Player(#[from] PlayerError),
    #[error("ALSA error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct GaplessEngine {
    playlist: Vec<PathBuf>,
    current_track: usize,
    player_state: Arc<Mutex<PlayerState>>,
    is_playing: bool,
    card: String,
    device_index: u32,
}

impl GaplessEngine {
    pub fn new(player_state: Arc<Mutex<PlayerState>>, card: String, device_index: u32) -> Self {
        Self {
            playlist: Vec::new(),
            current_track: 0,
            player_state,
            is_playing: false,
            card,
            device_index,
        }
    }

    pub fn add_to_playlist(&mut self, path: &Path) {
        self.playlist.push(path.to_path_buf());
    }

    fn open_device(&self) -> Result<BitPerfectDevice, alsa::Error> {
        if self.card == "default" {
            BitPerfectDevice::open("2", 0)
                .or_else(|_| BitPerfectDevice::open("1", 0))
                .or_else(|_| BitPerfectDevice::open("0", 0))
                .or_else(|_| BitPerfectDevice::open_raw("default"))
        } else {
            BitPerfectDevice::open(&self.card, self.device_index)
        }
    }

    pub fn play(&mut self) -> Result<(), GaplessError> {
        // Handle commands first
        {
            let mut state = self.player_state.lock().unwrap();
            if let Some(cmd) = state.command.take() {
                match cmd {
                    crate::gui::PlayerCommand::Next => {
                        if !self.playlist.is_empty() && self.current_track < self.playlist.len() - 1 {
                            self.current_track += 1;
                        } else if !self.playlist.is_empty() {
                            // Loop to beginning if at end
                            self.current_track = 0;
                        }
                    }
                    crate::gui::PlayerCommand::Prev => {
                        if self.current_track > 0 {
                            self.current_track -= 1;
                        }
                    }
                    crate::gui::PlayerCommand::PlayIndex(idx) => {
                        if idx < self.playlist.len() {
                            self.current_track = idx;
                        }
                    }
                }
                
                if !self.playlist.is_empty() {
                    self.is_playing = true;
                    state.is_playing = true;
                } else {
                    self.is_playing = false;
                    state.is_playing = false;
                }
            }
        }

        let (path, should_play) = {
            let state = self.player_state.lock().unwrap();
            if !state.is_playing || self.playlist.is_empty() || self.current_track >= self.playlist.len() {
                (None, false)
            } else {
                (Some(self.playlist[self.current_track].clone()), true)
            }
        };

        if !should_play {
            if (self.playlist.is_empty() || self.current_track >= self.playlist.len()) && self.is_playing {
                self.is_playing = false;
                let mut state = self.player_state.lock().unwrap();
                state.is_playing = false;
                state.current_track = None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            return Ok(());
        }

        let path = path.unwrap();
        
        // Open hardware for this track session
        let device = self.open_device()?;
        let mut player = BitPerfectPlayer::new(device);

        {
            let mut state = self.player_state.lock().unwrap();
            state.current_track = Some(crate::gui::TrackInfo {
                filename: path.file_name().unwrap().to_string_lossy().to_string(),
                sample_rate: 0,
                bit_depth: 0,
            });
            state.error_message = None; // Clear any old errors
        }
        
        player.play_file(&path, self.player_state.clone())?;
        
        // After track ends (or was stopped)
        let mut state = self.player_state.lock().unwrap();
        if state.is_playing && state.command.is_none() {
            self.current_track += 1;
            if self.current_track >= self.playlist.len() {
                state.is_playing = false;
                state.current_track = None;
            }
        }

        Ok(())
    }
}
