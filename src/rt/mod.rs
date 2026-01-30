use libc::{sched_setscheduler, SCHED_FIFO, sched_param};

#[derive(Debug, thiserror::Error)]
pub enum RtError {
    #[error("Permission denied to set real-time priority")]
    PermissionDenied,
}

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

pub fn pin_to_cpu(core_id: usize) {
    use libc::{cpu_set_t, CPU_SET, sched_setaffinity};
    
    let mut set: cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { CPU_SET(core_id, &mut set) };
    
    unsafe {
        sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &set);
    }
}

pub fn lock_memory() {
    // Prevent swapping of audio buffers
    unsafe {
        libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
    }
}