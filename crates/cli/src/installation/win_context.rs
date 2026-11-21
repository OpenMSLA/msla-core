use std::path::PathBuf;

use anyhow::{Result, anyhow};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use msla_core::SUPPORTED_FILE_EXTENSIONS;
use winreg::{RegKey, enums::*};

use crate::installation::APP_FOLDER_NAME;

/// Add context menu option for print start
pub async fn add_to_context(exe_path: PathBuf, label: &str) -> Result<()> {
    let search_options = [
        "Use broadcast (default)",
        "Use unicast (slower, but more reliable)",
    ];

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select printer search method")
        .items(search_options)
        .default(0)
        .interact()?;

    let port: u16 = Input::new()
        .with_prompt("Select scan port")
        .default(710)
        .interact_text()
        .map_err(|e| anyhow!("Cannot process scan port: {}", e))?;

    let alt_scan = if selection == 1 { "--alt-scan" } else { "" };
    let target_exe_str = exe_path.to_str().ok_or(anyhow!("Invalid exe path"))?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in SUPPORTED_FILE_EXTENSIONS {
        let key_path = format!(
            r"Software\Classes\SystemFileAssociations\.{}\shell\{}",
            ext, APP_FOLDER_NAME
        );
        let (key, _) = hkcu.create_subkey(&key_path)?;

        key.set_value("", &label)?;
        key.set_value("Icon", &target_exe_str)?;

        let (cmd_key, _) = key.create_subkey("command")?;

        let command = format!(
            r#""{}" {} --scan-port {} --from-context-menu start "%1""#,
            target_exe_str, alt_scan, port
        );
        cmd_key.set_value("", &command)?;
    }

    Ok(())
}

/// Remove file from context menu
pub async fn remove_from_context() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in SUPPORTED_FILE_EXTENSIONS {
        let key_path = format!(
            r"Software\Classes\SystemFileAssociations\.{}\shell\{}",
            ext, APP_FOLDER_NAME
        );
        let _ = hkcu.delete_subkey_all(&key_path);
    }

    Ok(())
}
