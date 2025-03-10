use crate::timer::ReportTiming;

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