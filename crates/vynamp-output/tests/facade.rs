//! The facade re-exports the core and selects a host.
//!
//! `OUTPUT-BK-AC-11` and `OUTPUT-BK-AC-12` were written when only the Null
//! host existed and say `hosts()` lists "só `Null`". `OUTPUT-AC-16`, approved
//! later the same day, has as its Given that the WASAPI host is "already in
//! the module's `hosts()` list". The two cannot both hold on Windows, so
//! these tests assert the later criterion on Windows and the earlier one
//! everywhere else, and the divergence is reported to the maintainer rather
//! than resolved by editing the spec.

use vynamp_output::*;

/// Whether this build contains the WASAPI backend.
fn wasapi_compiled() -> bool {
    cfg!(target_os = "windows")
}

#[test]
fn hosts_lists_null_only_where_no_os_backend_exists() {
    let ids: Vec<HostId> = hosts().iter().map(|h| h.id()).collect();
    if wasapi_compiled() {
        assert_eq!(ids, vec![HostId::Wasapi, HostId::Null], "got {ids:?}");
    } else {
        assert_eq!(ids, vec![HostId::Null], "got {ids:?}");
    }
}

#[test]
fn default_host_is_the_first_listed_and_says_so() {
    let (host, selection) = default_host();
    let first = hosts()[0].id();
    assert_eq!(host.id(), first);
    assert_eq!(selection.chosen, first);
    if wasapi_compiled() {
        assert_eq!(host.id(), HostId::Wasapi);
        assert_eq!(
            selection.skipped,
            vec![(HostId::Null, "fallback host: discards audio")]
        );
    } else {
        assert_eq!(host.id(), HostId::Null);
        assert_eq!(selection.skipped, Vec::new());
    }
}

#[test]
fn the_null_host_still_enumerates_its_virtual_device() {
    // Whatever else is listed, the Null host is always usable, which is why
    // it is the last resort on every target.
    let null = hosts()
        .into_iter()
        .find(|h| h.id() == HostId::Null)
        .expect("the Null host is always listed");
    assert_eq!(null.enumerate().unwrap().len(), 1);
}
