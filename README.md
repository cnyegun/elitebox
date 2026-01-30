# Elitebox 🎧

Elitebox is a high-fidelity, bit-perfect music player for Linux, engineered in Rust. It bypasses system sound servers (PulseAudio/PipeWire) to speak directly to your ALSA hardware, ensuring that your high-resolution music reaches your DAC exactly as intended—without resampling or mixing.

## ✨ Features

- **Bit-Perfect Playback**: Exclusive ALSA hardware access for bit-perfect audio delivery.
- **High-Resolution Support**: Native support for 24-bit and 32-bit FLAC, WAV, and more.
- **Zero-Copy Architecture**: Uses MMAP and direct I/O for minimal CPU overhead and jitter.
- **Suckless GUI**: A minimalist, high-information-density interface powered by `egui` and themed with Gruvbox.
- **Reactive Transport**: Near-instant response for play, pause, skip, and volume changes.
- **Real-Time Optimization**: Optional SCHED_FIFO thread priority and CPU pinning to eliminate audio dropouts.
- **Smart Hardware Discovery**: Automatically detects USB DACs (like the CX31993) for hassle-free setup.
- **Drag & Drop**: Drag files or entire folders directly into the player to build your playlist.

## 🛠 Prerequisites

To build and run Elitebox, you need the ALSA development headers installed on your system:

**Fedora/RedHat:**
```bash
sudo dnf install alsa-lib-devel
```

**Ubuntu/Debian:**
```bash
sudo apt-get install libasound2-dev
```

## 🚀 Quick Start

1. **Clone and Build:**
   ```bash
   git clone git@github.com:cnyegun/elitebox.git
   cd elitebox
   cargo build --release
   ```

2. **Run:**
   ```bash
   ./target/release/elitebox
   ```

3. **Usage:**
   - **Space**: Toggle Play/Pause
   - **J/K or Arrow Up/Down**: Navigate Browser
   - **L or Enter**: Enter Folder / Add song to Playlist
   - **H or Backspace**: Go to Parent Folder
   - **N / P**: Next / Previous Track
   - **Q**: Quit

## 🎛 Command Line Arguments

While the UI is fully functional without arguments, you can override defaults:

```bash
./target/release/elitebox --card 2 --device 0 /path/to/music
```

- `--card`: Explicitly set the ALSA card (index or name).
- `--device`: Set the sub-device index (default: 0).
- `--cpu`: Pin the audio thread to a specific CPU core (default: 0).

## ⚖️ License

Distributed under the MIT License. See `LICENSE` for more information.
