use alsa::pcm::PCM;

#[allow(dead_code)]
pub fn configure_software_params(pcm: &PCM) -> Result<(), alsa::Error> {
    let hwp = pcm.hw_params_current()?;
    let swp = pcm.sw_params_current()?;
    
    let buffer_size = hwp.get_buffer_size()?;
    // Disable ALSA's start threshold (we control when to start)
    swp.set_start_threshold(buffer_size)?;
    
    // Disable XRUN recovery (we handle underruns ourselves)
    // This is now done by setting the avail_min so that the interrupt is only generated when the buffer is almost empty.
    swp.set_avail_min(buffer_size / 2)?;
    
    // Disable timestamp interpolation (use real hardware timestamps)
    swp.set_tstamp_type(alsa::pcm::TstampType::MonotonicRaw)?;
    
    pcm.sw_params(&swp)?;
    Ok(())
}
