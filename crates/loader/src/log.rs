//! The loader's log file, started over each time the game starts.

use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);

pub fn open(path: &Path) {
    if let Ok(mut log) = LOG.lock() {
        *log = File::create(path).ok();
    }
}

pub fn line(text: fmt::Arguments) {
    if let Ok(mut log) = LOG.lock()
        && let Some(file) = log.as_mut()
    {
        let _ = write!(file, "{text}\r\n");
    }
}

pub fn flush() {
    if let Ok(mut log) = LOG.lock()
        && let Some(file) = log.as_mut()
    {
        let _ = file.sync_data();
    }
}

macro_rules! log {
    ($($part:tt)*) => {
        $crate::log::line(format_args!($($part)*))
    };
}
pub(crate) use log;
