use std::time::SystemTime;


use backtrace::Frame;
use smallvec::SmallVec;

use crate::{MAX_DEPTH, MAX_THREAD_NAME};
use crate::backtrace::{TraceImpl, Trace};
use crate::timer::ReportTiming;

pub struct Sample {
    pub(crate) backtrace: SmallVec<[<TraceImpl as Trace>::Frame; MAX_DEPTH]>,
    // pub(crate) thread_name: [u8; MAX_THREAD_NAME],
    pub(crate) thread_name: Vec<u8>,
    pub(crate) thread_id: u64,
    pub(crate) timestamp: SystemTime,
    pub(crate) count: i32,
}

#[no_mangle]
#[allow(clippy::unnecessary_cast)]
pub extern "C" fn sample_current() -> Sample {
    let current_thread = unsafe { libc::pthread_self() };
    let mut name = [0i8; MAX_THREAD_NAME];
    let name_ptr = &mut name as *mut [libc::c_char] as *mut libc::c_char;
    crate::profiler::write_thread_name(current_thread, &mut name);
    let thread_name = unsafe { std::ffi::CStr::from_ptr(name_ptr) };

    // println!("name is '{:?}'", name);
    // let name_bytes = name.to_owned().into_bytes();
    // let (first_16, _) = name_bytes.split_at(16);
    // let thread_name: [u8; MAX_THREAD_NAME] = first_16.try_into().unwrap();
    
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
        count: 1,
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


#[derive(Debug, Clone)]
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