mod cooldown_util;
mod favorites;
mod gpx_util;
mod handlers;
mod nominatim;
mod osrm;
mod profile_util;
mod server;
mod state;
mod ticker;
mod ws;

use clap::{Args, Parser, Subcommand};
use lc_device::ConnectOptions;
use state::AppState;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(
    name = "bloomanywhere",
    author,
    version,
    about = "BloomAnywhere: Cross-platform GPS simulator for Bloom games with web UI",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the web server and backend (default)
    Serve(ServeArgs),

    /// List connected devices (iOS, Android, and Mock)
    Devices(DevicesArgs),

    /// Set simulated location on a device
    Set(SetArgs),

    /// Restore real GPS location on a device
    Clear(ClearArgs),
}

#[derive(Args, Debug)]
struct ServeArgs {
    /// Host address to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8787)]
    port: u16,

    /// Do not automatically open web UI in default browser
    #[arg(long)]
    no_open: bool,

    /// Automatically open web UI in default browser (default)
    #[arg(long, conflicts_with = "no_open")]
    open: bool,

    /// Enable mock virtual device for testing/debugging
    #[arg(long)]
    mock: bool,

    /// Debug mode (enables mock virtual device)
    #[arg(long)]
    debug: bool,

    /// Disable mock device
    #[arg(long)]
    no_mock: bool,
}

impl Default for ServeArgs {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8787,
            no_open: false,
            open: true,
            mock: false,
            debug: false,
            no_mock: false,
        }
    }
}

#[derive(Args, Debug)]
struct DevicesArgs {
    /// Enable mock virtual device
    #[arg(long)]
    mock: bool,

    /// Debug mode (enables mock virtual device)
    #[arg(long)]
    debug: bool,

    /// Disable mock device
    #[arg(long)]
    no_mock: bool,
}

#[derive(Args, Debug)]
struct SetArgs {
    /// Target device ID (UDID, adb serial, or "mock"). Defaults to first device found.
    #[arg(short, long)]
    device: Option<String>,

    /// Target latitude
    #[arg(long)]
    lat: f64,

    /// Target longitude
    #[arg(long)]
    lng: f64,

    /// Enable mock virtual device
    #[arg(long)]
    mock: bool,

    /// Debug mode (enables mock virtual device)
    #[arg(long)]
    debug: bool,

    /// Disable mock device
    #[arg(long)]
    no_mock: bool,
}

#[derive(Args, Debug)]
struct ClearArgs {
    /// Target device ID (UDID, adb serial, or "mock"). Defaults to first device found.
    #[arg(short, long)]
    device: Option<String>,

    /// Enable mock virtual device
    #[arg(long)]
    mock: bool,

    /// Debug mode (enables mock virtual device)
    #[arg(long)]
    debug: bool,

    /// Disable mock device
    #[arg(long)]
    no_mock: bool,
}

fn should_enable_mock(mock: bool, debug: bool, no_mock: bool) -> bool {
    if no_mock {
        return false;
    }
    mock || debug
        || std::env::var("LC_ENABLE_MOCK")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        || std::env::var("LC_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bloomanywhere=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let command = cli
        .command
        .unwrap_or_else(|| Commands::Serve(ServeArgs::default()));

    match command {
        Commands::Serve(args) => {
            let opts = ConnectOptions {
                enable_mock: should_enable_mock(args.mock, args.debug, args.no_mock),
                ..Default::default()
            };

            let app_state = AppState::new(opts);
            let should_open = !args.no_open;
            server::run_server(&args.host, args.port, should_open, app_state).await?;
        }
        Commands::Devices(args) => {
            let opts = ConnectOptions {
                enable_mock: should_enable_mock(args.mock, args.debug, args.no_mock),
                ..Default::default()
            };

            let devices = lc_device::list_devices(&opts).await?;
            if devices.is_empty() {
                println!("No devices detected.");
            } else {
                println!(
                    "{:<20} {:<24} {:<10} {:<10} {:<12}",
                    "ID", "NAME", "PLATFORM", "OS", "CONNECTION"
                );
                println!("{:-<80}", "");
                for d in devices {
                    println!(
                        "{:<20} {:<24} {:<10?} {:<10} {:<12}",
                        d.id, d.name, d.platform, d.os_version, d.connection
                    );
                }
            }
        }
        Commands::Set(args) => {
            let opts = ConnectOptions {
                enable_mock: should_enable_mock(args.mock, args.debug, args.no_mock),
                ..Default::default()
            };

            let device_id = match args.device {
                Some(id) => id,
                None => {
                    let devices = lc_device::list_devices(&opts).await?;
                    devices
                        .into_iter()
                        .next()
                        .map(|d| d.id)
                        .ok_or_else(|| anyhow::anyhow!("No devices found"))?
                }
            };

            println!("Connecting to device {device_id}...");
            let mut session = lc_device::connect(&device_id, &opts).await?;
            println!(
                "Setting location to lat: {}, lng: {}...",
                args.lat, args.lng
            );
            session.set_location(args.lat, args.lng).await?;
            println!("Location successfully updated!");
        }
        Commands::Clear(args) => {
            let opts = ConnectOptions {
                enable_mock: should_enable_mock(args.mock, args.debug, args.no_mock),
                ..Default::default()
            };

            let device_id = match args.device {
                Some(id) => id,
                None => {
                    let devices = lc_device::list_devices(&opts).await?;
                    devices
                        .into_iter()
                        .next()
                        .map(|d| d.id)
                        .ok_or_else(|| anyhow::anyhow!("No devices found"))?
                }
            };

            println!("Connecting to device {device_id}...");
            let mut session = lc_device::connect(&device_id, &opts).await?;
            println!("Clearing location simulation...");
            session.clear().await?;
            println!("Real GPS location restored!");
        }
    }

    Ok(())
}
