impl super::Frame for backtrace::Frame {
    type S = backtrace::Symbol;

    fn resolve_symbol<F: FnMut(&Self::S)>(&self, cb: F) {
        backtrace::resolve_frame(self, cb);
    }
}

pub struct Trace {}

impl super::Trace for Trace {
    type Frame = backtrace::Frame;

    fn trace<F: FnMut(&Self::Frame) -> bool>(cb: F) {
        unsafe { backtrace::trace_unsynchronized(cb) }
    }
}
