//! Events produced by non-real-time threads.

use std::fmt;

use crate::config::StreamConfig;
use crate::request::{DeviceId, Role, StreamRequest};

/// Notifications about devices and streams.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    DeviceAdded(DeviceId),
    DeviceRemoved(DeviceId),
    DefaultChanged {
        role: Role,
    },
    DeviceLost,
    SharedFormatChanged,
    Downgraded {
        requested: Box<StreamRequest>,
        actual: Box<StreamConfig>,
        reason: &'static str,
    },
    OsXrun {
        count: u64,
    },
    SourceUnderrun {
        count: u64,
    },
    SourcePanicked,
    RtPriorityDenied {
        reason: &'static str,
    },
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Console => "console",
        Role::Multimedia => "multimedia",
        Role::Communications => "communications",
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::DeviceAdded(id) => write!(f, "device added: {}", id.0),
            Event::DeviceRemoved(id) => write!(f, "device removed: {}", id.0),
            Event::DefaultChanged { role } => {
                write!(f, "default device changed ({})", role_name(*role))
            }
            Event::DeviceLost => f.write_str("device lost"),
            Event::SharedFormatChanged => f.write_str("shared format changed"),
            Event::Downgraded {
                requested,
                actual,
                reason,
            } => write!(
                f,
                "downgraded: {reason} (requested {} Hz, {} ch; actual {} Hz, {} ch)",
                requested.rate, requested.channels, actual.rate, actual.channels
            ),
            Event::OsXrun { count } => write!(f, "os xrun (count {count})"),
            Event::SourceUnderrun { count } => write!(f, "source underrun (count {count})"),
            Event::SourcePanicked => f.write_str("source panicked"),
            Event::RtPriorityDenied { reason } => {
                write!(f, "realtime priority denied: {reason}")
            }
        }
    }
}
