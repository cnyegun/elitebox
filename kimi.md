# Hi-Fi Music Player in Rust - Project Plan

## Project Overview

Build a high-fidelity music player in Rust that achieves **bit-perfect playback** by using ALSA in direct hardware mode, bypassing all kernel software processing (dmix, resampling, software volume). The goal is to send the original audio bits unmodified to the DAC.

The music player's name is Elitebox

---

## What "Bypass the Kernel" Actually Means Here

### Standard Linux Audio Path (What We AVOID)

```
┌─────────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌─────┐
│  App Audio  │──▶│  dmix    │──▶│ resample │──▶│  plug    │──▶│  hw      │──▶│ DAC │
│  (32-bit)   │   │ (mixing) │   │ (rate)   │   │ (format) │   │ (driver) │   │     │
└─────────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └─────┘
      │                │               │              │              │
      ▼                ▼               ▼              ▼              ▼
  Bit-perfect?      NO             NO            Maybe          YES
```

### Our Bit-Perfect Path (What We WANT)

```
┌─────────────┐   ┌──────────┐   ┌─────┐
│  Our Player │──▶│  hw:0,0  │──▶│ DAC │
│  (file fmt) │   │ (direct) │   │     │
└─────────────┘   └──────────┘   └─────┘
      │                │              │
      ▼                ▼              ▼
  Bit-perfect?      YES            YES
```

### ALSA Device Types

| Device | Path | Bit-Perfect? | Use Case |
|--------|------|--------------|----------|
| `default` | Full ALSA chain | ❌ | General desktop audio |
| `plughw:0,0` | hw + auto format/rate | ❌ (converts) | Compatibility |
| `hw:0,0` | Direct to driver | ✅ | **Our target** |

---

## Phase 1: ALSA Exclusive Hardware Mode

### 1.1 Open ALSA in Direct HW Mode

```rust
// src/alsa/device.rs
use alsa::{Direction, ValueOr, pcm::{PCM, HwParams, Format, Access}};

pub struct BitPerfectDevice {
    pcm: PCM,
    current_format: Format,
}

impl BitPerfectDevice {
    pub fn open(card: &str, device: u32) -> Result<Self, AlsaError> {
        // Open hardware device directly - NO PLUGINS
        let pcm = PCM::new(card, Direction::Playback, false)?;
        
        // Get hardware parameters
        let hwp = HwParams::any(&pcm)?;
        
        // MUST set exclusive mode - blocks other apps
        hwp.set_access(Access::RWInterleaved)?;
        // Or use MMAP for zero-copy: hwp.set_access(Access::MMapInterleaved)?;
        
        Ok(Self { pcm, current_format: Format::Unknown })
    }
    
    /// Configure for exact file format - NO CONVERSION
    pub fn configure_exact(
        &mut self,
        sample_rate: u32,
        bit_depth: u16,
        channels: u8,
    ) -> Result<(), AlsaError> {
        let hwp = HwParams::any(&self.pcm)?;
        
        // Set EXACT format - fail if hardware doesn't support
        let format = match bit_depth {
            16 => Format::S16LE,
            24 => Format::S24_3LE,  // 24-bit packed (3 bytes)
            32 => Format::S32LE,
            _ => return Err(AlsaError::UnsupportedBitDepth),
        };
        
        hwp.set_format(format)?;
        hwp.set_channels(channels as u32)?;
        hwp.set_rate(sample_rate, ValueOr::Exact)?; // EXACT - no resampling
        
        // Apply to hardware
        self.pcm.hw_params(&hwp)?;
        self.current_format = format;
        
        // Log the actual hardware format for verification
        let actual_rate = hwp.get_rate()?;
        let actual_channels = hwp.get_channels()?;
        log::info!(
            "Hardware configured: {}Hz, {}ch, {:?}",
            actual_rate, actual_channels, format
        );
        
        Ok(())
    }
}
```

### 1.2 Disable All Software Processing

```rust
// src/alsa/sw_params.rs
use alsa::pcm::SwParams;

pub fn configure_software_params(pcm: &PCM) -> Result<(), AlsaError> {
    let swp = SwParams::new(pcm)?;
    
    // Disable ALSA's start threshold (we control when to start)
    swp.set_start_threshold(pcm.buffer_size().unwrap_or(0))?;
    
    // Disable XRUN recovery (we handle underruns ourselves)
    swp.set_xrun_mode(alsa::pcm::XRunMode::None)?;
    
    // Disable timestamp interpolation (use real hardware timestamps)
    swp.set_tstamp_mode(alsa::pcm::TstampMode::Enable)?;
    swp.set_tstamp_type(alsa::pcm::TstampType::MonotonicRaw)?;
    
    pcm.sw_params(&swp)?;
    Ok(())
}
```

### 1.3 Check System Configuration

```bash
# Check current ALSA config
cat /proc/asound/card0/pcm0p/sub0/hw_params

# Check for dmix (should show "closed" for exclusive mode)
cat /proc/asound/card0/pcm0p/sub0/status

# Disable system-wide dmix for bit-perfect
# Edit /etc/asound.conf or ~/.asoundrc:
```

```conf
# /etc/asound.conf - Disable dmix for card 0
pcm.!default {
    type hw
    card 0
    device 0
}

# Or create a dedicated bit-perfect device
pcm.bitperfect {
    type hw
    card 0
    device 0
}
```

---

## Phase 2: Zero-Copy Audio Path

### 2.1 MMAP-Based Playback

```rust
// src/alsa/mmap.rs
use alsa::pcm::{PCM, MmapPlayback, MmapPlaybackIO};

pub struct MmapPlayer {
    pcm: PCM,
    mmap: MmapPlayback<'static>,
}

impl MmapPlayer {
    pub fn new(pcm: PCM) -> Result<Self, AlsaError> {
        // Map DMA buffer directly into our address space
        let mmap = pcm.direct_mmap_playback::<i16>()?;
        
        Ok(Self { pcm, mmap })
    }
    
    /// Write directly to DMA buffer - ZERO COPY
    pub fn write_direct(&mut self, samples: &[i16]) -> Result<usize, AlsaError> {
        let avail = self.mmap.available();
        let to_write = samples.len().min(avail);
        
        // Direct memory write - no kernel copy!
        self.mmap.write(&samples[..to_write]);
        
        Ok(to_write)
    }
    
    /// Signal hardware to start reading
    pub fn commit(&mut self) -> Result<(), AlsaError> {
        // Update hardware pointer
        Ok(())
    }
}
```

### 2.2 File-to-DMA Direct Streaming

```rust
// src/player/bitperfect.rs
use std::fs::File;
use std::os::unix::fs::FileExt;

pub struct BitPerfectPlayer {
    device: BitPerfectDevice,
    decoder: Box<dyn Decoder>,
}

impl BitPerfectPlayer {
    pub fn play_file(&mut self, path: &Path) -> Result<(), PlayerError> {
        // 1. Open and analyze file
        let mut file = File::open(path)?;
        let format = self.probe_format(&file)?;
        
        // 2. Configure DAC to EXACT file format
        self.device.configure_exact(
            format.sample_rate,
            format.bit_depth,
            format.channels,
        )?;
        
        // 3. Stream directly - NO BUFFER CONVERSIONS
        let mut buffer: Vec<u8> = vec![0; format.block_size * 4];
        
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 { break; }
            
            // Write raw bytes directly to ALSA
            // The bytes are already in the exact format the DAC expects
            self.device.write_raw(&buffer[..bytes_read])?;
        }
        
        // 4. Drain and close
        self.device.drain()?;
        
        Ok(())
    }
}
```

---

## Phase 3: Format Handling

### 3.1 Supported Formats Matrix

| Format | Container | ALSA Format | Notes |
|--------|-----------|-------------|-------|
| CD Quality | WAV/FLAC | `S16LE` | 44.1kHz/16bit |
| Hi-Res 96/24 | WAV/FLAC | `S24_3LE` | 96kHz/24bit packed |
| Hi-Res 192/24 | WAV/FLAC | `S24_3LE` | 192kHz/24bit packed |
| Hi-Res 384/32 | WAV/FLAC | `S32LE` | 384kHz/32bit |
| DSD64 | DSF/DFF | `S32LE` | DoP (DSD over PCM) |
| DSD128 | DSF/DFF | `S32LE` | DoP |

### 3.2 DoP (DSD over PCM) Handler

```rust
// src/formats/dop.rs
/// Pack DSD samples into PCM frames for DoP-capable DACs
pub struct DoPEncoder {
    marker: u8, // 0x05, 0xFA pattern
}

impl DoPEncoder {
    /// Convert DSD bytes to 24-bit PCM frames
    /// Pattern: [0x05][DSD byte][0xFA][DSD byte] for DSD64
    pub fn encode(&self, dsd_bytes: &[u8]) -> Vec<u8> {
        let mut pcm = Vec::with_capacity(dsd_bytes.len() * 4);
        
        for (i, &byte) in dsd_bytes.iter().enumerate() {
            let marker = if i % 2 == 0 { 0x05 } else { 0xFA };
            // 24-bit little-endian: [byte][0x00][marker]
            pcm.push(byte);
            pcm.push(0x00);
            pcm.push(marker);
            pcm.push(0x00); // padding to 32-bit
        }
        
        pcm
    }
}
```

### 3.3 Format Converter (Only when hardware doesn't support)

```rust
// src/formats/converter.rs - LAST RESORT
/// Only convert if hardware truly doesn't support native format
pub struct FormatConverter;

impl FormatConverter {
    /// 24-bit packed (3 bytes) to 32-bit (4 bytes)
    /// This IS lossy in the sense of padding, but not in value
    pub fn s24_3le_to_s32le(input: &[u8]) -> Vec<u8> {
        assert!(input.len() % 3 == 0);
        let mut output = Vec::with_capacity(input.len() / 3 * 4);
        
        for chunk in input.chunks_exact(3) {
            // Sign-extend 24-bit to 32-bit
            let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]);
            let sign_extended = if sample & 0x800000 != 0 {
                sample | 0xFF000000
            } else {
                sample
            };
            output.extend_from_slice(&sign_extended.to_le_bytes());
        }
        
        output
    }
}
```

---

## Phase 4: Gapless Playback

### 4.1 Pre-buffering Strategy

```rust
// src/player/gapless.rs
pub struct GaplessEngine {
    current_track: TrackDecoder,
    next_track: Option<TrackDecoder>,
    ring_buffer: DmaRingBuffer,
}

impl GaplessEngine {
    /// Start decoding next track while current is playing
    pub fn prepare_next(&mut self, path: &Path) -> Result<(), Error> {
        let decoder = create_decoder(path)?;
        
        // Verify same format for true gapless
        if decoder.sample_rate() != self.current_track.sample_rate() {
            // Need to reconfigure hardware - will have gap
            log::warn!("Format change - gapless not possible");
        }
        
        self.next_track = Some(decoder);
        Ok(())
    }
    
    /// Seamlessly switch at end of current track
    pub fn transition(&mut self) {
        if let Some(next) = self.next_track.take() {
            // Just swap decoders - DMA continues uninterrupted
            self.current_track = next;
        }
    }
}
```

---

## Phase 5: Real-Time Optimization

### 5.1 Thread Priority

```rust
// src/rt/mod.rs
use libc::{sched_setscheduler, SCHED_FIFO, sched_param};

pub fn set_audio_thread_priority() -> Result<(), RtError> {
    // SCHED_FIFO: Real-time, first-in-first-out
    // Priority 99 is highest (use 90-95 for audio)
    let param = sched_param { sched_priority: 95 };
    
    let result = unsafe {
        sched_setscheduler(0, SCHED_FIFO, &param)
    };
    
    if result != 0 {
        return Err(RtError::PermissionDenied);
    }
    
    // Lock memory to prevent page faults
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    
    Ok(())
}
```

### 5.2 CPU Affinity

```rust
pub fn pin_to_cpu(core_id: usize) {
    use libc::{cpu_set_t, CPU_SET, sched_setaffinity};
    
    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_SET(core_id, &mut set) };
    
    unsafe {
        sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &set);
    }
}
```

### 5.3 Lock Memory

```rust
pub fn lock_memory() {
    // Prevent swapping of audio buffers
    unsafe {
        libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
    }
}
```

---

## Phase 6: Suckless GUI Design

### Philosophy

Following [suckless.org](https://suckless.org/philosophy/) principles:
- **Simplicity**: Code should be minimal and understandable
- **Focus**: Do one thing well - play music, nothing else
- **No Bloat**: No album art caching, no lyrics fetching, no "smart" recommendations
- **Configuration > Features**: Behavior defined at compile time or simple config
- **Resource Frugal**: GUI should use <5% CPU on a Pi 4

### GUI Options Comparison

| Approach | Pros | Cons | Suckless? |
|----------|------|------|-----------|
| **egui** | Immediate mode, single dependency, simple | Rust-only widgets | ✅ Best |
| **ratatui** | Terminal UI, extremely minimal | Requires terminal | ✅ Good |
| **GTK4** | Native look, mature | Heavy deps, complex | ⚠️ Okay |
| **Iced** | Elm architecture, clean | More boilerplate | ✅ Good |
| **Web UI** | Remote access | HTTP server overhead | ❌ No |

### Recommended: egui with Custom Theme

```rust
// src/gui/mod.rs
use eframe::egui;

pub struct SucklessPlayer {
    player: Arc<Mutex<PlayerState>>,
    current_dir: PathBuf,
    files: Vec<PathBuf>,
    playlist: Vec<PathBuf>,
    current_track: usize,
    show_help: bool,
}

impl eframe::App for SucklessPlayer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Suckless: No animations, no gradients, flat colors
        self.apply_suckless_theme(ctx);
        
        // Keyboard-first control (vim bindings)
        self.handle_input(ctx);
        
        egui::CentralPanel::default().show(ctx, |ui| {
            // Suckless: Maximum information density
            self.render_now_playing(ui);
            ui.separator();
            
            // Two-pane layout: files | playlist
            ui.horizontal(|ui| {
                ui.set_height(ui.available_height());
                
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.4);
                    self.render_file_browser(ui);
                });
                
                ui.separator();
                
                ui.vertical(|ui| {
                    self.render_playlist(ui);
                });
            });
        });
    }
}

impl SucklessPlayer {
    fn apply_suckless_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        
        // Suckless: High contrast, minimal chrome
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(0xeb, 0xdb, 0xb2)); // gruvbox light
        style.visuals.panel_fill = egui::Color32::from_rgb(0x1d, 0x20, 0x21); // gruvbox dark bg
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x28, 0x28, 0x28);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x45, 0x85, 0x88); // accent
        
        // Suckless: No rounded corners (waste of pixels)
        style.visuals.window_rounding = 0.0.into();
        style.visuals.widgets.inactive.rounding = 0.0.into();
        
        // Suckless: Compact spacing
        style.spacing.item_spacing = egui::vec2(4.0, 2.0);
        
        ctx.set_style(style);
    }
    
    fn handle_input(&mut self, ctx: &egui::Context) {
        // Vim-style bindings
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_playback();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::J)) || 
           ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.move_selection(1);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::K)) || 
           ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.move_selection(-1);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L)) || 
           ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            self.play_selected();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::H)) || 
           ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            self.go_to_parent();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Q) && i.modifiers.shift) {
            self.show_help = !self.show_help;
        }
    }
    
    fn render_now_playing(&self, ui: &mut egui::Ui) {
        let state = self.player.lock().unwrap();
        
        ui.horizontal(|ui| {
            // Suckless: Text only, no album art thumbnails
            ui.monospace("NOW PLAYING:");
            
            if let Some(ref track) = state.current_track {
                ui.label(egui::RichText::new(&track.filename)
                    .monospace()
                    .strong());
                ui.label(format!("{}Hz/{}bit", 
                    track.sample_rate, track.bit_depth));
            } else {
                ui.label("[stopped]");
            }
        });
        
        // Progress bar (thin, minimal)
        if state.is_playing {
            let progress = state.position_secs / state.duration_secs;
            ui.add(egui::ProgressBar::new(progress as f32)
                .desired_height(2.0)
                .text(format!("{:.0}:{:02.0}/{:.0}:{:02.0}",
                    state.position_secs / 60.0,
                    state.position_secs % 60.0,
                    state.duration_secs / 60.0,
                    state.duration_secs % 60.0)));
        }
        
        // Transport controls (minimal icons or text buttons)
        ui.horizontal(|ui| {
            if ui.button("⏮").clicked() { self.prev(); }
            if ui.button(if state.is_playing { "⏸" } else { "▶" }).clicked() {
                self.toggle_playback();
            }
            if ui.button("⏹").clicked() { self.stop(); }
            if ui.button("⏭").clicked() { self.next(); }
            
            ui.separator();
            
            // Volume (dB scale, not percentage)
            ui.add(egui::Slider::new(&mut state.volume_db, -60.0..=0.0)
                .text("dB")
                .show_value(true));
        });
    }
    
    fn render_file_browser(&mut self, ui: &mut egui::Ui) {
        ui.monospace(format!("📁 {}", self.current_dir.display()));
        ui.separator();
        
        egui::ScrollArea::vertical().show(ui, |ui| {
            // ".." entry
            if ui.selectable_label(false, "../").clicked() {
                self.go_to_parent();
            }
            
            // Files and directories
            for (idx, path) in self.files.iter().enumerate() {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                
                let label = if path.is_dir() {
                    format!("📂 {}/", name)
                } else if is_audio_file(path) {
                    format!("♫ {}", name)
                } else {
                    format!("   {}", name)
                };
                
                let is_selected = self.selected_idx == idx;
                if ui.selectable_label(is_selected, label).clicked() {
                    self.select_and_enter(idx);
                }
            }
        });
    }
    
    fn render_playlist(&mut self, ui: &mut egui::Ui) {
        ui.monospace(format!("PLAYLIST ({}/{})", 
            self.current_track + 1, 
            self.playlist.len()));
        ui.separator();
        
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, path) in self.playlist.iter().enumerate() {
                let name = path.file_stem()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                
                let is_current = idx == self.current_track;
                let text = if is_current {
                    format!("> {}", name) // ">" indicates currently playing
                } else {
                    format!("  {}", name)
                };
                
                if ui.selectable_label(is_current, text).clicked() {
                    self.play_index(idx);
                }
            }
        });
    }
}
```

### Alternative: Ratatui (Terminal UI)

For a true minimal approach - no X11/Wayland needed:

```rust
// src/tui/mod.rs
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, List, ListItem, Gauge},
    Terminal,
};

pub struct TuiPlayer {
    player: Arc<Mutex<PlayerState>>,
}

impl TuiPlayer {
    pub fn draw(&mut self, terminal: &mut Terminal) -> Result<(), Error> {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),  // Now playing
                    Constraint::Min(0),     // Browser/Playlist
                    Constraint::Length(1),  // Status bar
                ])
                .split(f.size());
            
            // Now playing block
            let now_playing = Block::default()
                .title(" elitebox ")
                .borders(Borders::ALL);
            f.render_widget(now_playing, chunks[0]);
            
            // ... etc
        })?;
        Ok(())
    }
}
```

### Configuration (Compile-Time)

Suckless style - config in source:

```rust
// src/config.rs (edit and recompile to change)
pub const DEFAULT_MUSIC_DIR: &str = "/home/user/Music";
pub const DEFAULT_VOLUME: f32 = -10.0; // dB
pub const BUFFER_SIZE_MS: u32 = 50;    // Low latency
pub const USE_MMAP: bool = true;
pub const THEME: Theme = Theme::Gruvbox;

pub enum Theme {
    Gruvbox,
    SolarizedDark,
    BlackWhite,
}
```

### Key Bindings (Vim-Style)

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `l` / `→` / `Enter` | Enter directory / Play file |
| `h` / `←` / `Backspace` | Go to parent directory |
| `Space` | Play/Pause |
| `n` | Next track |
| `p` | Previous track |
| `s` | Stop |
| `a` | Add to playlist |
| `d` | Delete from playlist |
| `c` | Clear playlist |
| `+` / `-` | Volume up/down |
| `m` | Mute |
| `q` | Quit |
| `?` / `F1` | Show help |

### Window Layout

```
┌─────────────────────────────────────────────────────────┐
│  elitebox 0.1.0                     hw:0,0 | 44.1k/16   │
├─────────────────────────────────────────────────────────┤
│ NOW PLAYING:                                            │
│ Artist - Album - Title.flac                             │
│ 44.1kHz/16bit | FLAC | 2:34/4:12              [-10dB]   │
│ [⏮] [⏸] [⏹] [⏭]    [══════════════>        ]          │
├────────────────────┬────────────────────────────────────┤
│ 📁 ~/Music         │ PLAYLIST (3/12)                    │
│ ../                │   01 - Track One.flac              │
│ 📂 Jazz/           │   02 - Track Two.flac              │
│ 📂 Classical/      │ > 03 - Track Three.flac            │
│ ♫ song1.flac       │   04 - Track Four.flac             │
│ ♫ song2.wav        │   ...                              │
│ ♫ song3.dsf        │                                    │
├────────────────────┴────────────────────────────────────┤
│ j/k:nav  l:play  a:add  space:pause  q:quit             │
└─────────────────────────────────────────────────────────┘
```

---

## Phase 7: Verification & Testing

### 6.1 Bit-Perfect Verification

```rust
// src/test/verify.rs
/// Verify output matches input exactly
pub fn verify_bitperfect(input: &[u8], output_capture: &[u8]) -> bool {
    input == output_capture
}

/// Check no dmix/resampler is active
pub fn check_alsa_status(card: u8) -> Result<AStatus, Error> {
    let path = format!("/proc/asound/card{}/pcm0p/sub0/hw_params", card);
    let content = std::fs::read_to_string(path)?;
    
    // Should show actual hardware params, not "BROKEN PIPE"
    // If dmix is active, it won't show file's exact format
    
    Ok(parse_status(&content))
}
```

### 6.2 Latency Measurement

```rust
pub struct LatencyMeasurer;

impl LatencyMeasurer {
    pub fn measure_roundtrip(&self) -> Duration {
        // Play a click, measure time to hear it back
        // Target: < 50ms for local playback
    }
}
```

---

## Implementation Roadmap

### Milestone 1: Basic ALSA HW Mode (1-2 weeks)
- [ ] Open `hw:0,0` with `alsa-rs` crate
- [ ] Configure exclusive mode
- [ ] Play 16/44.1 WAV file bit-perfect
- [ ] Verify with `/proc/asound` status

### Milestone 2: Format Support (2 weeks)
- [ ] FLAC decoder integration (symphonia)
- [ ] 24/96 and 24/192 support
- [ ] 32-bit float → 24-bit integer conversion (if needed)
- [ ] Gapless FLAC playback

### Milestone 3: Hi-Res & DSD (2 weeks)
- [ ] 384kHz PCM
- [ ] DoP encoding for DSD64/128
- [ ] Native DSD (if DAC supports via `iec958`)
- [ ] Sample rate switching without restart

### Milestone 4: Optimization (1-2 weeks)
- [ ] MMAP-based playback
- [ ] Real-time thread setup
- [ ] Memory locking
- [ ] CPU affinity

### Milestone 5: Suckless GUI (2-3 weeks)
- [ ] Basic playback controls (play/pause/stop/next/prev)
- [ ] File browser (directory-based, no database)
- [ ] Simple playlist (plain text/m3u)
- [ ] Volume control
- [ ] Current track info (text only, no album art)
- [ ] Keyboard-driven interface (vim-style bindings)

### Milestone 6: Network Features (optional)
- [ ] UPnP/DLNA renderer
- [ ] Web remote control
- [ ] Squeezebox emulation

---

## Dependencies

```toml
[dependencies]
# ALSA bindings
alsa = "0.9"

# Audio metadata
cpal = { version = "0.15", optional = true }  # For comparison/testing

# Decoding
symphonia = { version = "0.5", features = ["flac", "wav", "mp3", "aac"] }

# DSD support (if not handled by symphonia)
dsf = "0.2"

# Async runtime (for network)
tokio = { version = "1", features = ["rt-multi-thread"], optional = true }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", default-features = false, features = ["fmt"] }

# Error handling
thiserror = "1.0"

# CLI
clap = { version = "4", features = ["derive"] }

# GUI - choose one
## Option 1: egui (recommended - minimal, immediate mode)
eframe = { version = "0.28", default-features = false, features = ["glow", "default_fonts"] }
egui_extras = { version = "0.28", features = ["svg"] }

## Option 2: ratatui (terminal UI - most suckless)
# ratatui = "0.26"
# crossterm = "0.27"

## Option 3: Iced (alternative)
# iced = { version = "0.12", default-features = false, features = ["tiny-skia"] }

[features]
default = ["gui-egui"]
gui-egui = ["dep:eframe"]
gui-tui = ["dep:ratatui", "dep:crossterm"]
```

---

## ALSA Configuration for Bit-Perfect

### Disable System Sounds

```bash
# Stop PulseAudio (if running)
pulseaudio --kill

# Or disable PulseAudio autospawn
# ~/.config/pulse/client.conf
autospawn = no
```

### Create Bit-Perfect ALSA Config

```conf
# ~/.asoundrc or /etc/asound.conf

# Disable all software processing
pcm.!default {
    type hw
    card 0
    device 0
}

# Explicit bit-perfect device
pcm.bitperfect {
    type hw
    card 0
    device 0
}

ctl.!default {
    type hw
    card 0
}
```

### Check Configuration

```bash
# List hardware devices
aplay -L
aplay -l

# Test bit-perfect playback
aplay -D hw:0,0 -c 2 -f S16_LE -r 44100 test.wav

# Check actual hardware format
watch -n 0.5 cat /proc/asound/card0/pcm0p/sub0/hw_params
```

---

## Next Steps

1. **Confirm target hardware**: Raspberry Pi 4/5 or x86_64 PC with USB DAC
2. **Acquire test equipment**: Good headphones/speakers for listening tests
3. **Decide on GUI approach**:
   - **egui** (default): Desktop GUI, works on X11/Wayland
   - **ratatui**: Terminal UI, extremely minimal, SSH-friendly
4. **Set up ALSA configuration**: Disable dmix, create bit-perfect device
5. **Study existing code**:
   - `alsa-rs` crate examples
   - `symphonia` decoding examples
   - `egui` immediate mode patterns

---

## References

- [ALSA PCM Interface](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m.html)
- [ALSA Architecture](https://www.alsa-project.org/main/index.php/ALSA_Architecture)
- [Bit-Perfect Audio on Linux](https://archimago.blogspot.com/2018/01/musings-more-fun-with-digital-filters.html)
- [DoP Standard](https://dsd-guide.com/dop-open-standard)
