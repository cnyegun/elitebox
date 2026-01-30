use alsa::{Direction, ValueOr, pcm::{PCM, HwParams, Format, Access}};

#[allow(dead_code)]
pub struct BitPerfectDevice {
    pub pcm: PCM,
    pub current_format: Format,
}

#[allow(dead_code)]
impl BitPerfectDevice {
    pub fn open_raw(name: &str) -> Result<Self, alsa::Error> {
        let pcm = PCM::new(name, Direction::Playback, false)?;
        Ok(Self { pcm, current_format: Format::Unknown })
    }

    pub fn open(card: &str, device: u32) -> Result<Self, alsa::Error> {
        let name = format!("hw:{},{}", card, device);
        Self::open_raw(&name)
    }

    /// Configure for exact file format - NO CONVERSION
    pub fn configure_exact(
        &mut self,
        sample_rate: u32,
        bit_depth: u16,
        channels: u8,
    ) -> Result<(), alsa::Error> {
        // If the device is already running or in a weird state, drop it to reset
        let _ = self.pcm.drop();

        // Always start from 'any' to get a clean slate of hardware capabilities
        let hwp = HwParams::any(&self.pcm)?;
        
        // Use RWInterleaved for the write_raw/io_bytes path
        hwp.set_access(Access::RWInterleaved)?;

        // Try to set the best possible format for the given bit depth
        let format = match bit_depth {
            16 => Format::S16LE,
            24 => {
                if hwp.test_format(Format::S32LE).is_ok() {
                    Format::S32LE
                } else {
                    Format::S243LE
                }
            },
            32 => Format::S32LE,
            _ => return Err(alsa::Error::new("Unsupported bit depth", -22)),
        };
        
        hwp.set_format(format)?;
        hwp.set_channels(channels as u32)?;
        
        let actual_rate = hwp.set_rate_near(sample_rate, ValueOr::Nearest)?;
        
        // Apply ALL parameters to hardware at once
        self.pcm.hw_params(&hwp)?;
        self.current_format = format;
        
        Ok(())
    }
    pub fn write_raw(&self, data: &[u8]) -> Result<usize, alsa::Error> {
        let io = self.pcm.io_bytes();
        io.writei(data)
    }

    pub fn drain(&self) -> Result<(), alsa::Error> {
        self.pcm.drain()
    }
}




