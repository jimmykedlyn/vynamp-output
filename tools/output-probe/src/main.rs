//! `output-probe`: opens a host/device, plays a 440 Hz sine and prints the
//! real `StreamConfig`, timing samples, counters and the number of heap
//! allocations made on the device thread while the stream ran.
//!
//! ```text
//! output-probe [--host NAME] [--device ID] [--clock virtual|real]
//!              [--rate HZ] [--channels N] [--period-frames N] [--periods N]
//! ```
//!
//! Defaults: host `null`, the host's default device, virtual clock (a
//! deterministic run that finishes at once), 48000 Hz, 2 channels, 256-frame
//! periods, 200 periods. `--clock real` paces periods on wall-clock time.

use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use vynamp_output::*;
use vynamp_output_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// Number of timing samples printed.
const TIMING_SAMPLES: u64 = 5;

#[derive(Debug, Clone, PartialEq)]
struct Args {
    host: String,
    device: Option<String>,
    real_clock: bool,
    rate: u32,
    channels: u16,
    period_frames: u32,
    periods: u64,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            host: "null".into(),
            device: None,
            real_clock: false,
            rate: 48_000,
            channels: 2,
            period_frames: 256,
            periods: 200,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--host" => out.host = value("--host")?,
            "--device" => out.device = Some(value("--device")?),
            "--clock" => match value("--clock")?.as_str() {
                "real" => out.real_clock = true,
                "virtual" => out.real_clock = false,
                other => return Err(format!("unknown clock {other:?} (virtual or real)")),
            },
            "--rate" => out.rate = num(&value("--rate")?, "--rate")?,
            "--channels" => out.channels = num(&value("--channels")?, "--channels")?,
            "--period-frames" => {
                out.period_frames = num(&value("--period-frames")?, "--period-frames")?;
            }
            "--periods" => out.periods = num(&value("--periods")?, "--periods")?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if out.channels == 0 || out.period_frames == 0 || out.rate == 0 || out.periods == 0 {
        return Err("rate, channels, period-frames and periods must be positive".into());
    }
    Ok(out)
}

fn num<T: std::str::FromStr>(text: &str, name: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{name}: bad number {text:?}"))
}

/// Allocation counts of the device thread, read on the control thread.
#[derive(Default)]
struct AllocProbe {
    first: AtomicU64,
    last: AtomicU64,
    calls: AtomicU64,
}

/// A 440 Hz sine that records, on the device thread, how many allocations
/// happened between its first and its latest `render` call.
struct Sine {
    phase: f64,
    probe: Arc<AllocProbe>,
}

impl RenderSource for Sine {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let now = allocations_on_this_thread();
        if self.probe.calls.fetch_add(1, Ordering::Relaxed) == 0 {
            self.probe.first.store(now, Ordering::Relaxed);
        }
        self.probe.last.store(now, Ordering::Relaxed);

        let DeviceBuffer::F32(samples) = out else {
            return Rendered {
                frames: 0,
                end_of_stream: true,
            };
        };
        let step = std::f64::consts::TAU * 440.0 / f64::from(ctx.config.rate);
        let channels = usize::from(ctx.config.channels).max(1);
        let mut frames = 0;
        for frame in samples.chunks_exact_mut(channels).take(ctx.frames as usize) {
            frame.fill((self.phase.sin() * 0.25) as f32);
            self.phase = (self.phase + step) % std::f64::consts::TAU;
            frames += 1;
        }
        Rendered {
            frames,
            end_of_stream: false,
        }
    }
}

fn run(args: &[String], out: &mut dyn Write) -> Result<(), String> {
    let args = parse_args(args)?;
    let w = |e: std::io::Error| e.to_string();

    let virtual_clock = VirtualClockHandle::new();
    let host: Box<dyn Host> = match args.host.as_str() {
        "null" if args.real_clock => Box::new(NullHost::new(NullClock::Real)),
        "null" => Box::new(NullHost::new(NullClock::Virtual(virtual_clock.clone()))),
        other => {
            let names: Vec<_> = hosts().iter().map(|h| h.id().name()).collect();
            return Err(format!("unknown host {other:?}; available: {names:?}"));
        }
    };
    writeln!(out, "host: {}", host.id()).map_err(w)?;

    let device = match &args.device {
        Some(id) => DeviceSelector::Id(DeviceId(id.clone())),
        None => DeviceSelector::Default(Role::Multimedia),
    };
    let req = StreamRequest {
        device,
        mode: ShareMode::Shared,
        format: SampleFormat::F32,
        rate: args.rate,
        channels: args.channels,
        layout: ChannelLayout::unknown(args.channels),
        buffer: BufferRequest::Frames(args.period_frames),
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    };
    let probe = Arc::new(AllocProbe::default());
    let source = Sine {
        phase: 0.0,
        probe: Arc::clone(&probe),
    };
    let mut stream = host
        .open(&req, Box::new(source))
        .map_err(|e| e.to_string())?;
    writeln!(out, "config: {:?}", stream.config()).map_err(w)?;

    stream.start().map_err(|e| e.to_string())?;
    let chunk = (args.periods / TIMING_SAMPLES).max(1);
    let wall = Duration::from_secs_f64(
        chunk as f64 * f64::from(args.period_frames) / f64::from(args.rate),
    );
    for i in 0..TIMING_SAMPLES {
        if args.real_clock {
            std::thread::sleep(wall);
        } else {
            virtual_clock.advance(chunk);
        }
        match stream.timing() {
            Some(t) => writeln!(
                out,
                "timing[{i}]: device_frame={} latency_frames={} source={:?} at_ns={}",
                t.device_frame, t.latency_frames, t.source, t.at.0
            ),
            None => writeln!(out, "timing[{i}]: none"),
        }
        .map_err(w)?;
    }
    stream.stop().map_err(|e| e.to_string())?;

    let c = stream.counters();
    writeln!(
        out,
        "counters: callbacks={} frames_delivered={} source_underruns={} os_xruns={}",
        c.callbacks, c.frames_delivered, c.source_underruns, c.os_xruns
    )
    .map_err(w)?;
    let calls = probe.calls.load(Ordering::Relaxed);
    let allocs = probe.last.load(Ordering::Relaxed) - probe.first.load(Ordering::Relaxed);
    writeln!(
        out,
        "device-thread allocations: {allocs} (between the first and the last of {calls} render calls)"
    )
    .map_err(w)?;
    for event in stream.events().try_iter() {
        writeln!(out, "event: {event}").map_err(w)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args, &mut std::io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("output-probe: {e}");
            ExitCode::from(2)
        }
    }
}
