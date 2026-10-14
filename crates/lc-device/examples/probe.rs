//! CLI device probe and test tool.
//!
//! Usage:
//!   cargo run -p lc-device --example probe
//!   cargo run -p lc-device --example probe -- --id mock --lat 25.0330 --lng 121.5654
//!   cargo run -p lc-device --example probe -- --id <device_id> --no-mock

use lc_device::{connect, list_devices, ConnectOptions};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return Ok(());
    }

    let mut target_id = None;
    let mut enable_mock = true;
    let mut lat = 25.0330;
    let mut lng = 121.5654;
    let mut clear_after = true;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--id" if i + 1 < args.len() => {
                target_id = Some(args[i + 1].clone());
                i += 1;
            }
            "--no-mock" => {
                enable_mock = false;
            }
            "--lat" if i + 1 < args.len() => {
                lat = args[i + 1].parse().unwrap_or(25.0330);
                i += 1;
            }
            "--lng" if i + 1 < args.len() => {
                lng = args[i + 1].parse().unwrap_or(121.5654);
                i += 1;
            }
            "--no-clear" => {
                clear_after = false;
            }
            _ => {}
        }
        i += 1;
    }

    println!("============================================================");
    println!("  BloomAnywhere (lc-device) Probe CLI   ");
    println!("============================================================");
    println!();

    let opts = ConnectOptions {
        enable_mock,
        ..Default::default()
    };

    println!("Scanning connected devices (iOS via usbmuxd, Android via adb, Mock)...");
    let devices = list_devices(&opts).await?;

    if devices.is_empty() {
        println!("No devices detected.");
        return Ok(());
    }

    println!("\nFound {} device(s):", devices.len());
    for (idx, dev) in devices.iter().enumerate() {
        let platform_tag = match dev.platform {
            lc_device::Platform::Ios => "iOS    ",
            lc_device::Platform::Android => "Android",
            lc_device::Platform::Mock => "Mock   ",
        };
        println!(
            "  [{}] {:7} | Name: {:<20} | OS: {:<8} | Model: {:<16} | Conn: {:<10} | ID: {}",
            idx + 1,
            platform_tag,
            dev.name,
            dev.os_version,
            dev.model,
            dev.connection,
            dev.id
        );
    }
    println!();

    // Determine target device
    let chosen_id = match target_id {
        Some(id) => id,
        None => {
            // Default to first non-mock device if available, otherwise mock
            devices
                .iter()
                .find(|d| d.platform != lc_device::Platform::Mock)
                .map(|d| d.id.clone())
                .unwrap_or_else(|| "mock".to_string())
        }
    };

    println!("Connecting to target device: '{chosen_id}' ...");
    match connect(&chosen_id, &opts).await {
        Ok(mut session) => {
            let info = session.info();
            println!(
                "Successfully connected to {} ({}) [Platform: {:?}, OS: {}]",
                info.name, info.id, info.platform, info.os_version
            );

            println!("\nTesting set_location to lat={lat:.6}, lng={lng:.6} ...");
            match session.set_location(lat, lng).await {
                Ok(()) => {
                    println!("Location set successfully!");
                }
                Err(e) => {
                    eprintln!("Failed to set location: {e:#}");
                }
            }

            println!("Testing ping ...");
            match session.ping().await {
                Ok(()) => println!("Ping OK"),
                Err(e) => eprintln!("Ping failed: {e:#}"),
            }

            if clear_after {
                println!("\nClearing location simulation (restoring real GPS)...");
                match session.clear().await {
                    Ok(()) => println!("Location cleared successfully!"),
                    Err(e) => eprintln!("Failed to clear location: {e:#}"),
                }
            } else {
                println!("\nKeeping location simulation active (--no-clear).");
            }
        }
        Err(e) => {
            eprintln!("\nFailed to connect to device '{chosen_id}':\n{e:#}");
        }
    }

    println!("\nProbe completed.");
    Ok(())
}

fn print_usage() {
    println!("Usage: probe [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --id <ID>       Target device UDID, adb serial, or 'mock' (default: first device)");
    println!("  --no-mock       Exclude mock device from scanning");
    println!("  --lat <LAT>     Test latitude (default: 25.0330)");
    println!("  --lng <LNG>     Test longitude (default: 121.5654)");
    println!("  --no-clear      Do not clear simulated location on exit");
    println!("  -h, --help      Show this help message");
}
