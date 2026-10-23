use clap::{Parser, Subcommand};

// cli start --zip --local "file.zip"
// cli start --zip --remote "rem-file.zip"
// cli start --other-ext --local "file.ext"

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start print
    Start(StartArgs),

    /// Abort print
    Abort,

    /// Pause print
    Pause,

    /// Resume print
    Resume,

    /// Get current printer status
    Status(StatusArgs),

    /// Home Z axis
    Home,

    /// Disable Z stepper
    DisableStepper,

    /// Show model preview
    ShowPreview,

    /// Add "Send to printer" context option
    ContextRegister,

    /// Remove "Send to printer" context option
    ContextUnregister,

    /// Display model info
    ModelInfo(ModelInfoArgs),

    /// Change cli configuration
    ChangeConfig(ConfigChange),

    /// Move Z axis to
    MoveTo(MoveTo),
}

#[derive(Parser, Debug)]
pub struct MoveTo {
    #[arg(long, help = "Position in mm")]
    pub pos: f64,

    #[arg(long, help = "Speed in mm/s")]
    pub speed: Option<f64>,
}

#[derive(Parser, Debug)]
pub struct ModelInfoArgs {
    #[arg(long, help = "Path to the model")]
    pub path: String,
}

#[derive(Parser, Debug)]
pub struct StartArgs {
    #[arg(long, conflicts_with = "remote", help = "Path to file on printer", conflicts_with_all = ["local"])]
    pub local: Option<String>,

    #[arg(long, conflicts_with = "local", help = "Path to file on this device", conflicts_with_all = ["remote"])]
    pub remote: Option<String>,

    #[arg(long, help = "Do not shutdown status")]
    pub watch: bool,
}

#[derive(Parser, Debug)]
pub struct StatusArgs {
    #[arg(long, help = "Do not shutdown status, and update them every <period>")]
    pub watch: bool,

    #[arg(long, help = "Printer polling interval")]
    pub period: Option<u64>,
}

#[derive(Parser, Debug)]
pub struct ConfigChange {
    #[arg(long, help = "Always use alternative scan method")]
    pub always_alt_scan: Option<String>,

    #[arg(long, help = "Default printer port")]
    pub default_port: Option<u16>,

    #[arg(long, help = "Default printer scan port")]
    pub default_scan_port: Option<u16>,
}

#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, help = "Specify printer IP/host")]
    pub host: Option<String>,

    #[arg(long, help = "Use unicast search method (broadcast by default)")]
    pub alt_scan: bool,

    #[arg(long, help = "Scanning port (710 by default)")]
    pub scan_port: Option<u16>,

    #[arg(long, help = "Specify printer port")]
    pub port: Option<u16>,

    #[arg(long, help = "Print is started from context menu")]
    pub from_context_menu: bool,
}
