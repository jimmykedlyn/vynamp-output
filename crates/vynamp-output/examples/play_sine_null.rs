//! Plays a 440 Hz sine for about 200 ms on the default (Null) host and
//! prints the negotiated config, the timing and the counters.

use std::time::Duration;

use vynamp_output::*;

/// A sine generator. Real-time safe: no allocation, locks or I/O.
struct Sine {
    phase: f64,
    step: f64,
}

impl RenderSource for Sine {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let DeviceBuffer::F32(samples) = out else {
            return Rendered {
                frames: 0,
                end_of_stream: true,
            };
        };
        let channels = usize::from(ctx.config.channels).max(1);
        let mut frames = 0;
        for frame in samples.chunks_exact_mut(channels).take(ctx.frames as usize) {
            frame.fill((self.phase.sin() * 0.25) as f32);
            self.phase = (self.phase + self.step) % std::f64::consts::TAU;
            frames += 1;
        }
        Rendered {
            frames,
            end_of_stream: false,
        }
    }
}

fn main() -> Result<(), Error> {
    let (host, selection) = default_host();
    println!(
        "host: {} (skipped: {:?})",
        selection.chosen, selection.skipped
    );
    let rate = 48_000;
    let req = StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Shared,
        format: SampleFormat::F32,
        rate,
        channels: 2,
        layout: ChannelLayout::unknown(2),
        buffer: BufferRequest::BackendDefault,
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    };
    let sine = Sine {
        phase: 0.0,
        step: std::f64::consts::TAU * 440.0 / f64::from(rate),
    };
    let mut stream = host.open(&req, Box::new(sine))?;
    println!("config: {:#?}", stream.config());
    stream.start()?;
    std::thread::sleep(Duration::from_millis(200));
    println!("timing: {:?}", stream.timing());
    stream.stop()?;
    println!("counters: {:?}", stream.counters());
    for event in stream.events().try_iter() {
        println!("event: {event}");
    }
    Ok(())
}
