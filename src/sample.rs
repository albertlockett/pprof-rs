use std::time::SystemTime;

use smallvec::SmallVec;

use crate::backtrace::{Trace, TraceImpl};
use crate::timer::ReportTiming;
use crate::{MAX_DEPTH, MAX_THREAD_NAME};

pub struct Sample {
    pub(crate) backtrace: SmallVec<[<TraceImpl as Trace>::Frame; MAX_DEPTH]>,
    pub(crate) thread_name: Vec<u8>,
    pub(crate) thread_id: u64,
    pub(crate) timestamp: SystemTime,
    pub(crate) count: isize,
}

#[inline]
#[no_mangle]
#[allow(clippy::unnecessary_cast)]
pub fn sample_current(count: isize) -> Sample {
    let current_thread = unsafe { libc::pthread_self() };
    let mut name = [0i8; MAX_THREAD_NAME];
    let name_ptr = &mut name as *mut [libc::c_char] as *mut libc::c_char;
    write_thread_name(current_thread, &mut name);
    let thread_name = unsafe { std::ffi::CStr::from_ptr(name_ptr) };

    let mut bt: SmallVec<[<TraceImpl as Trace>::Frame; MAX_DEPTH]> =
        SmallVec::with_capacity(MAX_DEPTH);
    let mut index = 0;

    TraceImpl::trace(|frame| {
        #[cfg(feature = "frame-pointer")]
        {
            let ip = crate::backtrace::Frame::ip(frame);
            if profiler.is_blocklisted(ip) {
                return false;
            }
        }

        if index < MAX_DEPTH {
            bt.push(frame.clone());
            index += 1;
            true
        } else {
            false
        }
    });

    Sample {
        backtrace: bt,
        thread_name: thread_name.to_owned().into(),
        thread_id: current_thread as u64,
        timestamp: SystemTime::now(),
        count,
    }
}

fn write_thread_name_fallback(current_thread: libc::pthread_t, name: &mut [libc::c_char]) {
    let mut len = 0;
    let mut base = 1;

    while current_thread as u128 > base && len < MAX_THREAD_NAME {
        base *= 10;
        len += 1;
    }

    let mut index = 0;
    while index < len && base > 1 {
        base /= 10;

        name[index] = match (48 + (current_thread as u128 / base) % 10).try_into() {
            Ok(digit) => digit,
            Err(_) => {
                log::error!("fail to convert thread_id to string");
                0
            }
        };

        index += 1;
    }
}

#[cfg(not(all(any(target_os = "linux", target_os = "macos"), target_env = "gnu")))]
fn write_thread_name(current_thread: libc::pthread_t, name: &mut [libc::c_char]) {
    write_thread_name_fallback(current_thread, name);
}

#[cfg(all(any(target_os = "linux", target_os = "macos"), target_env = "gnu"))]
fn write_thread_name(current_thread: libc::pthread_t, name: &mut [libc::c_char]) {
    let name_ptr = name as *mut [libc::c_char] as *mut libc::c_char;
    let ret = unsafe { libc::pthread_getname_np(current_thread, name_ptr, MAX_THREAD_NAME) };

    if ret != 0 {
        write_thread_name_fallback(current_thread, name);
    }
}

const COUNT: &str = "count";
const NANOSECONDS: &str = "nanoseconds";
const SAMPLES: &str = "samples";
const CPU: &str = "cpu";

impl SampleTypes {
    pub fn new<T>(sample_descriptions: T) -> Self
    where
        T: IntoIterator<Item = SampleType>,
    {
        Self {
            descriptions: sample_descriptions.into_iter().collect(),
        }
    }
}

impl Default for SampleTypes {
    /// Default for CPU sampled profile
    fn default() -> Self {
        Self {
            descriptions: vec![
                SampleType {
                    ty: SAMPLES.into(),
                    unit: Unit::Count,
                },
                SampleType {
                    ty: CPU.into(),
                    unit: Unit::Nanoseconds,
                },
            ],
        }
    }
}

/// A description of a sample type.
/// For example, for a CPU prpfile it might be:
/// `SampleType { ty: "cpu", unit: "nanoseconds" }`
/// For a heap profile it might be:
/// `SampleType { ty: "allocations", unit: "count" }`
/// For a syscall profile it might be:
/// `SampleType { ty: "syscall", unit: "count" }`
/// etc.
#[derive(Debug, Clone)]
pub struct SampleType {
    /// Description of the sample type. E.g. "cpu", "wall", "allocations", "syscall", etc.
    pub ty: String,
    pub unit: Unit,
}

impl SampleType {
    pub fn new(ty: String, unit: Unit) -> Self {
        Self { ty, unit }
    }
}

#[derive(Debug, Clone)]
pub struct SampleTypes {
    pub descriptions: Vec<SampleType>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Unit {
    Count,
    Nanoseconds,
}

impl Unit {
    pub fn to_sample_value(&self, count: i64, timing: &ReportTiming) -> i64 {
        match self {
            Unit::Count => count,
            Unit::Nanoseconds => count * 1_000_000_000 / timing.frequency as i64,
        }
    }
}

impl From<&Unit> for String {
    fn from(unit: &Unit) -> Self {
        match unit {
            Unit::Count => COUNT.into(),
            Unit::Nanoseconds => NANOSECONDS.into(),
        }
    }
}
