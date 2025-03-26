// Copyright 2019 TiKV Project Authors. Licensed under Apache-2.0.

use std::os::raw::c_int;

use nix::sys::signal;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

#[cfg(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "loongarch64"
))]
use findshlibs::{Segment, SharedLibrary, TargetSharedLibrary};

use crate::collector::Collector;
use crate::error::{Error, Result};
use crate::frames::UnresolvedFrames;
use crate::report::ReportBuilder;
use crate::sample::{sample_current, Sample};
use crate::timer::Timer;

pub(crate) static PROFILER: Lazy<RwLock<Result<Profiler>>> =
    Lazy::new(|| RwLock::new(Profiler::new()));

pub struct Profiler {
    pub(crate) data: Collector<UnresolvedFrames>,
    sample_counter: i32,

    running: bool,

    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ))]
    blocklist_segments: Vec<(usize, usize)>,
}

#[derive(Clone)]
pub struct ProfilerGuardBuilder {
    frequency: c_int,
    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ))]
    blocklist_segments: Vec<(usize, usize)>,
}

impl Default for ProfilerGuardBuilder {
    fn default() -> ProfilerGuardBuilder {
        ProfilerGuardBuilder {
            frequency: 99,

            #[cfg(any(
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64"
            ))]
            blocklist_segments: Vec::new(),
        }
    }
}

impl ProfilerGuardBuilder {
    pub fn frequency(self, frequency: c_int) -> Self {
        Self { frequency, ..self }
    }

    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ))]
    pub fn blocklist<T: AsRef<str>>(self, blocklist: &[T]) -> Self {
        let blocklist_segments = {
            let mut segments = Vec::new();
            TargetSharedLibrary::each(|shlib| {
                let in_blocklist = match shlib.name().to_str() {
                    Some(name) => {
                        let mut in_blocklist = false;
                        for blocked_name in blocklist.iter() {
                            if name.contains(blocked_name.as_ref()) {
                                in_blocklist = true;
                            }
                        }

                        in_blocklist
                    }

                    None => false,
                };
                if in_blocklist {
                    for seg in shlib.segments() {
                        let avam = seg.actual_virtual_memory_address(shlib);
                        let start = avam.0;
                        let end = start + seg.len();
                        segments.push((start, end));
                    }
                }
            });
            segments
        };

        Self {
            blocklist_segments,
            ..self
        }
    }
    pub fn build(self) -> Result<ProfilerGuard<'static>> {
        trigger_lazy();

        match PROFILER.write().as_mut() {
            Err(err) => {
                log::error!("Error in creating profiler: {}", err);
                Err(Error::CreatingError)
            }
            Ok(profiler) => {
                #[cfg(any(
                    target_arch = "x86_64",
                    target_arch = "aarch64",
                    target_arch = "riscv64",
                    target_arch = "loongarch64"
                ))]
                {
                    profiler.blocklist_segments = self.blocklist_segments;
                }

                match profiler.start() {
                    Ok(()) => Ok(ProfilerGuard::<'static> {
                        profiler: &PROFILER,
                        timer: Some(Timer::new(self.frequency)),
                    }),
                    Err(err) => Err(err),
                }
            }
        }
    }
}

/// RAII structure used to stop profiling when dropped. It is the only interface to access profiler.
pub struct ProfilerGuard<'a> {
    profiler: &'a RwLock<Result<Profiler>>,
    timer: Option<Timer>,
}

fn trigger_lazy() {
    let _ = backtrace::Backtrace::new();
    let _profiler = PROFILER.read();
}

impl ProfilerGuard<'_> {
    /// Start profiling with given sample frequency.
    pub fn new(frequency: c_int) -> Result<ProfilerGuard<'static>> {
        ProfilerGuardBuilder::default().frequency(frequency).build()
    }

    /// Generate a report
    pub fn report(&self) -> ReportBuilder {
        ReportBuilder::new(
            self.profiler,
            self.timer.as_ref().map(Timer::timing).unwrap_or_default(),
            None,
        )
    }
}

impl Drop for ProfilerGuard<'_> {
    fn drop(&mut self) {
        drop(self.timer.take());

        match self.profiler.write().as_mut() {
            Err(_) => {}
            Ok(profiler) => match profiler.stop() {
                Ok(()) => {}
                Err(err) => log::error!("error while stopping profiler {}", err),
            },
        }
    }
}

struct ErrnoProtector(libc::c_int);

impl ErrnoProtector {
    fn new() -> Self {
        unsafe {
            #[cfg(target_os = "android")]
            {
                let errno = *libc::__errno();
                Self(errno)
            }
            #[cfg(target_os = "linux")]
            {
                let errno = *libc::__errno_location();
                Self(errno)
            }
            #[cfg(any(target_os = "macos", target_os = "freebsd"))]
            {
                let errno = *libc::__error();
                Self(errno)
            }
        }
    }
}

impl Drop for ErrnoProtector {
    fn drop(&mut self) {
        unsafe {
            #[cfg(target_os = "android")]
            {
                *libc::__errno() = self.0;
            }
            #[cfg(target_os = "linux")]
            {
                *libc::__errno_location() = self.0;
            }
            #[cfg(any(target_os = "macos", target_os = "freebsd"))]
            {
                *libc::__error() = self.0;
            }
        }
    }
}

#[no_mangle]
#[cfg_attr(
    not(all(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ))),
    allow(unused_variables)
)]
#[allow(clippy::unnecessary_cast)]
extern "C" fn perf_signal_handler(
    _signal: c_int,
    _siginfo: *mut libc::siginfo_t,
    ucontext: *mut libc::c_void,
) {
    let _errno = ErrnoProtector::new();

    if let Some(mut guard) = PROFILER.try_write() {
        if let Ok(profiler) = guard.as_mut() {
            #[cfg(any(
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64"
            ))]
            if !ucontext.is_null() {
                let ucontext: *mut libc::ucontext_t = ucontext as *mut libc::ucontext_t;

                #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
                let addr =
                    unsafe { (*ucontext).uc_mcontext.gregs[libc::REG_RIP as usize] as usize };

                #[cfg(all(target_arch = "x86_64", target_os = "freebsd"))]
                let addr = unsafe { (*ucontext).uc_mcontext.mc_rip as usize };

                #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
                let addr = unsafe {
                    let mcontext = (*ucontext).uc_mcontext;
                    if mcontext.is_null() {
                        0
                    } else {
                        (*mcontext).__ss.__rip as usize
                    }
                };

                #[cfg(all(
                    target_arch = "aarch64",
                    any(target_os = "android", target_os = "linux")
                ))]
                let addr = unsafe { (*ucontext).uc_mcontext.pc as usize };

                #[cfg(all(target_arch = "aarch64", target_os = "freebsd"))]
                let addr = unsafe { (*ucontext).mc_gpregs.gp_elr as usize };

                #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
                let addr = unsafe {
                    let mcontext = (*ucontext).uc_mcontext;
                    if mcontext.is_null() {
                        0
                    } else {
                        (*mcontext).__ss.__pc as usize
                    }
                };

                #[cfg(all(target_arch = "riscv64", target_os = "linux"))]
                let addr = unsafe { (*ucontext).uc_mcontext.__gregs[libc::REG_PC] as usize };

                #[cfg(all(target_arch = "loongarch64", target_os = "linux"))]
                let addr = unsafe { (*ucontext).uc_mcontext.__pc as usize };

                if profiler.is_blocklisted(addr) {
                    return;
                }
            }

            let sample = sample_current(1);
            profiler.sample(sample);
        }
    }
}

impl Profiler {
    pub fn new() -> Result<Self> {
        Ok(Profiler {
            data: Collector::new()?,
            sample_counter: 0,
            running: false,

            #[cfg(any(
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64"
            ))]
            blocklist_segments: Vec::new(),
        })
    }

    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    ))]
    fn is_blocklisted(&self, addr: usize) -> bool {
        for libs in &self.blocklist_segments {
            if addr > libs.0 && addr < libs.1 {
                return true;
            }
        }
        false
    }
}

impl Profiler {
    pub fn start(&mut self) -> Result<()> {
        log::info!("starting cpu profiler");
        if self.running {
            Err(Error::Running)
        } else {
            self.register_signal_handler()?;
            self.running = true;

            Ok(())
        }
    }

    fn init(&mut self) -> Result<()> {
        self.sample_counter = 0;
        self.data = Collector::new()?;
        self.running = false;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        log::info!("stopping cpu profiler");
        if self.running {
            self.unregister_signal_handler()?;
            self.init()?;

            Ok(())
        } else {
            Err(Error::NotRunning)
        }
    }

    fn register_signal_handler(&self) -> Result<()> {
        let handler = signal::SigHandler::SigAction(perf_signal_handler);
        let sigaction = signal::SigAction::new(
            handler,
            // SA_RESTART will only restart a syscall when it's safe to do so,
            // e.g. when it's a blocking read(2) or write(2). See man 7 signal.
            signal::SaFlags::SA_SIGINFO | signal::SaFlags::SA_RESTART,
            signal::SigSet::empty(),
        );
        unsafe { signal::sigaction(signal::SIGPROF, &sigaction) }?;

        Ok(())
    }

    fn unregister_signal_handler(&self) -> Result<()> {
        let handler = signal::SigHandler::SigIgn;
        unsafe { signal::signal(signal::SIGPROF, handler) }?;

        Ok(())
    }

    // This function has to be AS-safe
    pub fn sample(&mut self, sample: Sample) {
        let frames = UnresolvedFrames::new(
            sample.backtrace,
            &sample.thread_name,
            sample.thread_id,
            sample.timestamp,
        );
        self.sample_counter += 1;

        if let Ok(()) = self.data.add(frames, sample.count) {}
    }
}
