# Elitebox 🎧

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Linux](https://img.shields.io/badge/platform-Linux-lightgrey.svg)](https://www.kernel.org/)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

**Elitebox** is a high-fidelity, bit-perfect music player for Linux, engineered in Rust. It is designed for audiophiles who demand the shortest possible path between their high-resolution audio files and their DAC.

*Note: Elitebox is written by Google Gemini 3 Flash Preview free plan.*

---

## 🎯 Why Elitebox?

Most Linux music players send audio through sound servers like **PipeWire** or **PulseAudio**. While convenient, these servers often:
- **Resample** your 192kHz files down to 48kHz.
- **Mix** audio with system sounds (notifications, browser audio).
- **Add Jitter** through software buffering layers.

**Elitebox bypasses the mess.** It uses **ALSA Exclusive Hardware Mode** to lock your DAC at its native frequency and bit depth, sending raw samples directly to the hardware buffer.

## ✨ Features

- **🛡️ Bit-Perfect Playback**: Exclusive hardware access ensures zero software interference.
- **💎 High-Resolution Support**: Native handling of 24-bit and 32-bit FLAC, WAV, and more.
- **⚡ Zero-Copy Architecture**: Utilizes MMAP and direct DMA buffer writes for minimal CPU jitter.
- **🧩 Suckless GUI**: A minimalist, high-density interface powered by `egui` with a Gruvbox-inspired theme.
- **🏎️ Reactive Transport**: Instant response for all controls via a lock-free reactive state model.
- **🌀 Real-Time Optimization**: Optional `SCHED_FIFO` thread priority and CPU pinning.
- **🔍 Smart Discovery**: Automatic detection of USB DACs (e.g., CX31993) on startup.
- **📁 Fluid Import**: Recursive folder scanning and drag-and-drop support.

## 🛠 Prerequisites

To compile Elitebox, you need ALSA development headers and `pkg-config` installed:

### **Fedora / RedHat / Nobara**
```bash
sudo dnf install alsa-lib-devel pkg-config
```

### **Ubuntu / Debian / Mint**
```bash
sudo apt-get install libasound2-dev pkg-config
```

### **Arch Linux**
```bash
sudo pacman -S alsa-lib
```

## 🚀 Quick Start

### 1. Build from Source
```bash
git clone git@github.com:cnyegun/elitebox.git
cd elitebox
cargo build --release
```

### 2. Run
```bash
./target/release/elitebox
```

## ⌨️ Controls

| Key | Action |
| :--- | :--- |
| `Space` | Toggle Play / Pause |
| `J` / `K` | Move selection Down / Up |
| `L` / `Enter` | Enter Folder / Add to Playlist |
| `H` / `Backspace` | Go to Parent Folder |
| `N` / `P` | Next / Previous Track |
| `S` | Stop Playback |
| `Q` | Quit |

## 🎛 Advanced Usage

Elitebox is smart enough to find your DAC automatically, but you can force specific hardware:

```bash
# Force specific ALSA card and device
./target/release/elitebox --card 2 --device 0

# Pin the audio engine to a specific CPU core to minimize context switching
./target/release/elitebox --cpu 3
```

### Real-Time Priority
To enable `SCHED_FIFO` (Real-Time) priority without `sudo`, add your user to the `audio` group and update `/etc/security/limits.conf`:
```text
@audio - rtprio 95
@audio - memlock unlimited
```

## ⚖️ License

Distributed under the GPL-3.0 License. See `LICENSE` for more information.
