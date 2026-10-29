//! Runs the probe binary on the Null host with the virtual clock.

use std::process::Command;

fn probe(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_output-probe"))
        .args(args)
        .output()
        .expect("run output-probe")
}

#[test]
fn null_virtual_run_prints_the_expected_shape() {
    let out = probe(&["--periods", "100", "--period-frames", "128"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "host: null");
    assert!(
        lines[1].starts_with("config: StreamConfig {"),
        "{}",
        lines[1]
    );
    assert!(lines[1].contains("rate: 48000"));
    for i in 0..5 {
        assert!(
            lines[2 + i].starts_with(&format!("timing[{i}]: device_frame=")),
            "{}",
            lines[2 + i]
        );
    }
    // 5 chunks of 20 periods of 128 frames.
    assert_eq!(
        lines[7],
        "counters: callbacks=100 frames_delivered=12800 source_underruns=0 os_xruns=0"
    );
    assert!(
        lines[8].starts_with(
            "device-thread allocations: 0 (between the first and the last of 100 render calls)"
        ),
        "{}",
        lines[8]
    );
}

#[test]
fn bad_arguments_fail_with_a_message() {
    let out = probe(&["--host", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown host"));
}
