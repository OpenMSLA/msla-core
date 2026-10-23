use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use tokio::fs;
use tracing::info;
use winreg::{RegKey, enums::*};

const APP_FOLDER_NAME: &str = "OpenMSLA";
const TARGET_EXE_NAME: &str = "msla-cli.exe";
const EXTENSIONS: [&str; 2] = ["zip", "photon"];

fn get_installed_exe_path() -> Result<PathBuf> {
    let local_app_data = env::var("LOCALAPPDATA")?;
    Ok(Path::new(&local_app_data)
        .join(APP_FOLDER_NAME)
        .join(TARGET_EXE_NAME))
}

/// Add context menu option for print start
pub async fn add_to_context(label: &str) -> Result<()> {
    let exe_path = env::current_exe()?;
    let target_exe = get_installed_exe_path()?;
    let target_dir = target_exe.parent().ok_or(anyhow!("Error APPDATA path"))?;

    if !target_dir.exists() {
        fs::create_dir_all(target_dir).await?;
    }

    if exe_path != target_exe {
        fs::copy(&exe_path, &target_exe).await?;
    }

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
    let target_exe_str = target_exe.to_str().ok_or(anyhow!("Invalid exe path"))?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in EXTENSIONS {
        let key_path = format!(
            r"Software\Classes\SystemFileAssociations\.{}\shell\{}",
            ext, APP_FOLDER_NAME
        );
        let (key, _) = hkcu.create_subkey(&key_path)?;

        key.set_value("", &label)?;
        key.set_value("Icon", &target_exe_str)?;

        let (cmd_key, _) = key.create_subkey("command")?;

        let command = format!(
            r#""{}" {} --scan-port {} --from-context-menu start --remote "%1""#,
            target_exe_str, alt_scan, port
        );
        cmd_key.set_value("", &command)?;
    }

    info!("Menu option is registered!");
    Ok(())
}

/// Remove file from context menu
pub async fn remove_from_context() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in EXTENSIONS {
        let key_path = format!(
            r"Software\Classes\SystemFileAssociations\.{}\shell\{}",
            ext, APP_FOLDER_NAME
        );
        let _ = hkcu.delete_subkey_all(&key_path);
    }

    if let Ok(target_exe) = get_installed_exe_path() {
        let current_exe = env::current_exe().ok();

        if current_exe.as_ref() != Some(&target_exe) {
            if target_exe.exists() {
                let _ = fs::remove_file(&target_exe).await;
            }
            if let Some(target_dir) = target_exe.parent() {
                let _ = fs::remove_dir(target_dir).await;
            }
        }
    }

    info!("Menu option is unregistered");
    Ok(())
}
