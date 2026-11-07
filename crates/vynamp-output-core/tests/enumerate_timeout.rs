//! `enumerate_with_timeout` with a deliberately blocking fake host.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vynamp_output_core::*;

/// A host whose `enumerate` blocks until the test releases it.
struct Blocking {
    gate: Mutex<Receiver<()>>,
}

impl Host for Blocking {
    fn id(&self) -> HostId {
        HostId::Null
    }
    fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error> {
        let _ = self.gate.lock().unwrap().recv();
        Ok(Vec::new())
    }
    fn default_device(&self, _role: Role) -> Result<Option<DeviceInfo>, Error> {
        Err(Error::NoBackend)
    }
    fn capabilities(&self, _d: &DeviceId, _m: ShareMode) -> Result<Capabilities, Error> {
        Err(Error::NoBackend)
    }
    fn open(&self, _r: &StreamRequest, _s: Box<dyn RenderSource>) -> Result<Stream, Error> {
        Err(Error::NoBackend)
    }
}

#[test]
fn blocking_enumeration_times_out_and_the_thread_is_counted() {
    let (release, gate) = channel();
    let host = Arc::new(Blocking {
        gate: Mutex::new(gate),
    });
    let before = abandoned_enumeration_threads();
    let r = enumerate_with_timeout(host, Duration::from_millis(20));
    assert_eq!(r, Err(Error::Timeout));
    assert_eq!(abandoned_enumeration_threads(), before + 1);
    drop(release); // lets the abandoned thread finish
}

#[test]
fn fast_enumeration_returns_the_devices() {
    let host: Arc<dyn Host> = Arc::new(NullHost::new(NullClock::Real));
    let devices = enumerate_with_timeout(host, Duration::from_secs(5)).unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].host, HostId::Null);
}
