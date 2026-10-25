use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DaemonDynConfigType {
    pub leveling: Leveling,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Leveling {
    #[serde(default)]
    pub bed_level_offset: f64,
}
