//! Structured errors.

use std::fmt;

use crate::config::StreamConfig;
use crate::format::HostId;

/// Errors reported by the library. Never a bare string.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    LibraryUnavailable {
        lib: &'static str,
    },
    NoBackend,
    DeviceNotFound,
    DeviceBusy,
    ExclusiveUnavailable {
        reason: &'static str,
    },
    FormatUnsupported {
        nearest: Option<Box<StreamConfig>>,
    },
    PermissionDenied,
    DeviceLost,
    Timeout,
    InvalidRequest {
        what: &'static str,
    },
    Os {
        backend: HostId,
        code: i64,
        call: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::LibraryUnavailable { lib } => write!(f, "library unavailable: {lib}"),
            Error::NoBackend => f.write_str("no audio backend available"),
            Error::DeviceNotFound => f.write_str("device not found"),
            Error::DeviceBusy => f.write_str("device busy"),
            Error::ExclusiveUnavailable { reason } => {
                write!(f, "exclusive mode unavailable: {reason}")
            }
            Error::FormatUnsupported { nearest: None } => f.write_str("format unsupported"),
            Error::FormatUnsupported { nearest: Some(n) } => {
                write!(
                    f,
                    "format unsupported (nearest: {} Hz, {} ch)",
                    n.rate, n.channels
                )
            }
            Error::PermissionDenied => f.write_str("permission denied"),
            Error::DeviceLost => f.write_str("device lost"),
            Error::Timeout => f.write_str("timeout"),
            Error::InvalidRequest { what } => write!(f, "invalid request: {what}"),
            Error::Os {
                backend,
                code,
                call,
            } => write!(f, "os error {code} in {call} ({backend})"),
        }
    }
}

impl std::error::Error for Error {}
