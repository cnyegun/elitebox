mod alsa;
mod player;
mod rt;
mod gui;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;

use clap::Parser;
use eframe::egui;

use crate::alsa::device::BitPerfectDevice;
use crate::player::bitperfect::BitPerfectPlayer;
use crate::player::gapless::GaplessEngine;
use crate::rt::{set_audio_thread_priority, pin_to_cpu, lock_memory};
use crate::gui::{SucklessPlayer, PlayerState};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The files to play (optional)
    #[arg()]
    files: Vec<PathBuf>,

    /// The ALSA card to use
    #[arg(long, default_value = "default")]
    card: String,

    /// The ALSA device to use
    #[arg(long, default_value = "0")]
    device: u32,

    /// The CPU core to pin the audio thread to
    #[arg(long, default_value = "0")]
    cpu: usize,
}

fn main() -> Result<(), eframe::Error> {
    let args = Args::parse();
    let (tx, rx) = mpsc::channel();
    let player_state = Arc::new(Mutex::new(PlayerState {
        current_track: None,
        is_playing: false,
        position_secs: 0.0,
        duration_secs: 0.0,
        volume_db: -10.0,
        playlist: Vec::new(),
        command: None,
        error_message: None,
        album_art: None,
    }));

    let player_state_audio = player_state.clone();
    thread::spawn(move || {
        let mut engine = GaplessEngine::new(player_state_audio.clone(), args.card.clone(), args.device);

        for file in args.files {
            engine.add_to_playlist(&file);
        }

        if let Err(e) = set_audio_thread_priority() {
            eprintln!("Warning: Failed to set audio thread priority: {}. Try running with sudo.", e);
        }
        pin_to_cpu(args.cpu);
        lock_memory();

        loop {
            match rx.try_recv() {
                Ok(msg) => match msg {
                    crate::gui::GuiMessage::AddToPlaylist(path) => {
                        engine.add_to_playlist(&path);
                        let mut state = player_state_audio.lock().unwrap();
                        state.playlist.push(path);
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
            
            if let Err(e) = engine.play() {
                // If it's a hardware error, report it and wait
                let msg = format!("Playback Error: {}. Retrying...", e);
                if let Ok(mut state) = player_state_audio.lock() {
                    state.error_message = Some(msg);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "elitebox",
        native_options,
        Box::new(|_cc| Ok(Box::new(SucklessPlayer::new(tx, player_state)))),
    )
}
