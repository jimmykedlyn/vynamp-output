//! The facade re-exports the core and selects the Null host for now.

use vynamp_output::*;

#[test]
fn hosts_lists_null_only() {
    let ids: Vec<HostId> = hosts().iter().map(|h| h.id()).collect();
    assert_eq!(ids, vec![HostId::Null]);
}

#[test]
fn default_host_is_null_and_says_so() {
    let (host, selection) = default_host();
    assert_eq!(host.id(), HostId::Null);
    assert_eq!(
        selection,
        HostSelection {
            chosen: HostId::Null,
            skipped: Vec::new()
        }
    );
    let devices = host.enumerate().unwrap();
    assert_eq!(devices.len(), 1);
}
