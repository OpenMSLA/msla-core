use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CliDynConfigType {
    pub connectivity: Connectivity,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Connectivity {
    #[serde(default)]
    pub alt_default: bool,

    #[serde(default = "default_port")]
    pub default_port: u16,

    #[serde(default = "default_scan_port")]
    pub default_scan_port: u16,
}

fn default_port() -> u16 {
    709
}

fn default_scan_port() -> u16 {
    710
}
