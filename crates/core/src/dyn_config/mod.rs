use std::{
    io::ErrorKind,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::RwLock};

pub struct DynConfig<T> {
    inner: OnceLock<Arc<RwLock<T>>>,
}

impl<T> DynConfig<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Default,
{
    /// Create new dyn config
    pub const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    /// Get path of dynamic user config
    fn get_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap()
            .join("OpenMSLA")
            .join("config.toml")
    }

    /// Acquires config
    pub fn config(&'static self) -> Result<&'static Arc<RwLock<T>>> {
        self.inner.get().ok_or(anyhow!("Config not loaded"))
    }

    /// Save config into file
    pub async fn write_config(&self, conf: &T) -> Result<()> {
        let path = Self::get_config_path();
        fs::create_dir_all(path.parent().unwrap()).await?;
        fs::write(path, toml::to_string_pretty(conf)?).await?;

        Ok(())
    }

    fn set(&self, t: T) -> Result<()> {
        self.inner
            .set(Arc::new(RwLock::new(t)))
            .map_err(|_| anyhow!("Config already initialized"))?;

        Ok(())
    }

    /// Inits the config
    pub async fn init_config(&self) -> Result<()> {
        let config = match fs::read_to_string(Self::get_config_path()).await {
            Ok(content) => {
                let value: toml::Value = toml::from_str(&content)?;
                let config: T = value.clone().try_into()?;

                let normalized = toml::to_string_pretty(&config)?;
                let original = toml::to_string_pretty(&value)?;

                if normalized != original {
                    self.write_config(&config).await?;
                }

                config
            },

            Err(e) if e.kind() == ErrorKind::NotFound => {
                let config = T::default();

                self.write_config(&config).await?;
                config
            },

            Err(e) => return Err(e.into()),
        };

        self.set(config)
    }

    /// Saves current config to a file
    pub async fn store_config(&'static self) -> Result<()> {
        let conf = &*self.config()?.read().await;
        self.write_config(conf).await?;

        Ok(())
    }
}

#[macro_export]
macro_rules! modify_config {
    ($config:expr, $($field:ident).+ = $value:expr) => {{
        let mut config = $config.config()?.write().await;
        config.$($field).+ = $value;
    }};
}
