//! Which hosts the facade lists, and in what order.
//!
//! `OUTPUT-AC-16`'s Given says the WASAPI host is "already in the module's
//! `hosts()` list", so that part is checkable here, on any target: on
//! Windows the list must contain WASAPI ahead of Null, and on every other
//! target it must not contain a backend that was not compiled.

use vynamp_output::*;

/// Whether this build contains the WASAPI backend.
fn wasapi_compiled() -> bool {
    cfg!(target_os = "windows")
}

#[test]
fn wasapi_is_listed_first_on_windows_only() {
    let ids: Vec<HostId> = hosts().iter().map(|h| h.id()).collect();
    if wasapi_compiled() {
        assert_eq!(
            ids.first(),
            Some(&HostId::Wasapi),
            "the real backend is preferred over the Null host, got {ids:?}"
        );
    } else {
        assert!(
            !ids.contains(&HostId::Wasapi),
            "a backend that was not compiled must not be listed, got {ids:?}"
        );
    }
    // The Null host is always the last resort and is never absent.
    assert_eq!(ids.last(), Some(&HostId::Null), "got {ids:?}");
}

#[test]
fn the_list_has_no_duplicates() {
    let ids: Vec<HostId> = hosts().iter().map(|h| h.id()).collect();
    let mut seen = ids.clone();
    seen.sort_by_key(|h| h.name());
    seen.dedup();
    assert_eq!(seen.len(), ids.len(), "duplicate host in {ids:?}");
}

#[test]
fn default_host_prefers_wasapi_and_says_what_it_skipped() {
    let (host, selection) = default_host();
    if wasapi_compiled() {
        assert_eq!(host.id(), HostId::Wasapi);
        assert_eq!(selection.chosen, HostId::Wasapi);
    } else {
        assert_eq!(host.id(), HostId::Null);
        assert_eq!(selection.chosen, HostId::Null);
        assert!(
            selection.skipped.is_empty(),
            "nothing was skipped on this target, got {:?}",
            selection.skipped
        );
    }
}

#[test]
fn support_declares_wasapi_as_compiled_and_never_stronger() {
    // The backend has never run on hardware, so `Compiled` — "the target
    // builds; nothing ran" — is the strongest honest claim. The answer must
    // not depend on the host OS, because `docs/support-matrix.md` is one
    // committed file generated on whichever machine runs the generator.
    for feature in Feature::ALL {
        assert_eq!(
            support(HostId::Wasapi, feature),
            Some(VerificationLevel::Compiled),
            "{feature:?}: no WASAPI test has run on hardware, so nothing \
             stronger than `Compiled` is honest"
        );
    }
}

#[test]
fn the_other_os_backends_are_not_implemented() {
    for host in [
        HostId::Alsa,
        HostId::PipeWire,
        HostId::Pulse,
        HostId::CoreAudio,
        HostId::AAudio,
    ] {
        for feature in Feature::ALL {
            assert_eq!(support(host, feature), None, "{host:?} {feature:?}");
        }
    }
}
