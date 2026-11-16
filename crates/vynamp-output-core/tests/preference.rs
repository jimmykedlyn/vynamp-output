//! Host preference order: which backend the facade tries first, and why the
//! others were skipped.
//!
//! The decision is a pure function of "which backends this build contains",
//! so it is testable on every target even though the Windows backend only
//! runs on Windows. The spec's rule is that a more specific backend beats a
//! more generic one, and that the Null host is always the last resort.

use vynamp_output_core::preference::{Preference, candidates};

#[test]
fn windows_lists_wasapi_first_then_null() {
    let p = candidates(&Preference::new(true, false, false, false));
    assert_eq!(p, vec!["wasapi", "null"]);
}

#[test]
fn a_build_without_wasapi_lists_only_null() {
    let p = candidates(&Preference::new(false, false, false, false));
    assert_eq!(p, vec!["null"]);
}

#[test]
fn capture_is_never_in_the_preference_list() {
    // `CaptureHost` is an internal test backend; it must never be chosen
    // for real playback even when it is compiled in.
    let p = candidates(&Preference::new(true, true, false, false));
    assert!(!p.contains(&"capture"), "got {p:?}");
}

#[test]
fn null_is_always_last_and_present_once() {
    for has_wasapi in [false, true] {
        for has_alsa in [false, true] {
            let p = candidates(&Preference::new(has_wasapi, false, has_alsa, false));
            assert_eq!(p.last(), Some(&"null"), "got {p:?}");
            assert_eq!(p.iter().filter(|h| **h == "null").count(), 1, "got {p:?}");
        }
    }
}

#[test]
fn a_backend_that_is_not_compiled_is_not_listed() {
    let p = candidates(&Preference::new(true, false, false, false));
    for absent in ["alsa", "pipewire", "pulse", "coreaudio", "aaudio"] {
        assert!(
            !p.contains(&absent),
            "{absent} listed without being compiled"
        );
    }
}

#[test]
fn the_order_is_stable_for_a_given_set() {
    let a = candidates(&Preference::new(true, false, true, false));
    let b = candidates(&Preference::new(true, false, true, false));
    assert_eq!(a, b);
}
