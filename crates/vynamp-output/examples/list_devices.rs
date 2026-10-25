//! Lists every host, its devices and their capabilities.

use vynamp_output::*;

fn main() -> Result<(), Error> {
    let (_, selection) = default_host();
    println!(
        "default host: {} (skipped: {:?})",
        selection.chosen, selection.skipped
    );
    for host in hosts() {
        println!("host {}", host.id());
        for device in host.enumerate()? {
            println!(
                "  {} [{}] id={:?} ({:?}) default for {:?}, transport {:?}",
                device.name,
                device.host,
                device.id.0,
                device.id_stability,
                device.default_for,
                device.transport
            );
            for mode in [ShareMode::Shared, ShareMode::Exclusive] {
                let caps = host.capabilities(&device.id, mode)?;
                for e in caps.entries {
                    println!(
                        "    {mode:?} {:?}: rates {:?}, channels {:?} ({:?})",
                        e.format, e.rates, e.channels, e.origin
                    );
                }
            }
        }
    }
    Ok(())
}
