#[cfg(target_os = "windows")]
mod win_context;

use std::env;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use anyhow::Error;
use colored::Colorize;
use dialoguer::Confirm;
use tokio::fs;

#[cfg(target_os = "windows")]
use win_context::add_to_context;

#[cfg(target_os = "windows")]
use win_context::remove_from_context;

#[cfg(not(target_os = "windows"))]
async fn add_to_context(label: &str) -> Result<()> {
    panic!("Context options not supported on this OS yet")
}

#[cfg(not(target_os = "windows"))]
async fn remove_from_context() -> Result<()> {
    panic!("Context options not supported on this OS yet")
}

use add_to_path::{add_to_path, remove_from_path};
use anyhow::{Result, anyhow};
mod add_to_path;

const APP_FOLDER_NAME: &str = "OpenMSLA";

#[cfg(windows)]
fn get_installed_path() -> Result<PathBuf> {
    const TARGET_EXE_NAME_WIN: &str = "msla-cli.exe";
    let local_app_data = env::var("LOCALAPPDATA")?;
    Ok(Path::new(&local_app_data)
        .join(APP_FOLDER_NAME)
        .join(TARGET_EXE_NAME_WIN))
}

#[cfg(target_os = "linux")]
fn get_installed_path() -> Result<PathBuf> {
    const TARGET_EXE_NAME_UNIX: &str = "msla-cli";
    let local_home = env::var("HOME")?;
    Ok(Path::new(&local_home)
        .join(".local")
        .join("bin")
        .join(TARGET_EXE_NAME_UNIX))
}

pub async fn install() -> Result<()> {
    let exe_path = env::current_exe()?;
    let target_exe = get_installed_path()?;
    let target_dir = target_exe
        .parent()
        .ok_or(anyhow!("Error installation path"))?;

    let install_confirmed = Confirm::new()
        .with_prompt(format!(
            "OpenMSLA CLI will be installed in: {}. Continue?",
            target_dir.to_string_lossy().bold()
        ))
        .default(true)
        .interact()
        .unwrap();

    if !install_confirmed {
        println!("Installation cancelled");
        return Ok(());
    }

    let install_result = async {
        if !target_dir.exists() {
            fs::create_dir_all(target_dir).await?;
        }

        if exe_path != target_exe {
            fs::copy(&exe_path, &target_exe).await?;
        }

        Ok::<(), Error>(())
    }
    .await;

    match install_result {
        Ok(()) => {
            println!("{}", "INSTALLED\n".green());
        },
        Err(e) => {
            println!("{}", "FAILED\n".red());
            eprint!("{e}");

            return Err(e);
        },
    }
    let confirmed = Confirm::new()
        .with_prompt("Add the installed CLI to the PATH environment variable?")
        .default(true)
        .interact()
        .unwrap();

    if !confirmed {
        println!("{}", "Installation finished".green());
        return Ok(());
    }

    print!("Adding to PATH");
    match add_to_path(target_dir.to_path_buf()).await {
        Ok(()) => {
            println!(": {}", "DONE\n".green());
        },
        Err(e) => {
            println!(": {}", "FAILED\n".red());
            eprint!("{e}");

            return Err(e);
        },
    }

    if cfg!(target_os = "windows") {
        let confirmed = Confirm::new()
            .with_prompt("Add \"Send to printer\" button to a context menu?")
            .default(true)
            .interact()
            .unwrap();

        if confirmed {
            match add_to_context(target_exe, "Send to printer").await {
                Ok(()) => {
                    println!("{}", "DONE\n".green());
                },
                Err(e) => {
                    println!("{}", "FAILED\n".red());
                    eprint!("{e}");

                    return Err(e);
                },
            }
        }
    }

    println!("{}", "Installation finished".green());
    Ok(())
}

pub async fn uninstall() -> Result<()> {
    let current_exe = env::current_exe().ok();
    let target_exe = get_installed_path()?;
    let target_dir = target_exe
        .parent()
        .ok_or(anyhow!("Error installation path"))?;

    if current_exe.as_ref() != Some(&target_exe) {
        if target_exe.exists() {
            let _ = fs::remove_file(&target_exe).await;
        }

        if let Some(target_dir) = target_exe.parent() {
            let _ = fs::remove_dir(target_dir).await;
        }
    }

    print!("Removing from PATH");
    match remove_from_path(target_dir.to_path_buf()).await {
        Ok(()) => {
            println!(": {}", "DONE".green());
        },
        Err(e) => {
            println!(": {}", "FAILED".red());
            eprint!("{e}");
        },
    }

    if cfg!(target_os = "windows") {
        print!("Removing from context menu");
        match remove_from_context().await {
            Ok(()) => {
                println!(": {}", "DONE\n".green());
            },
            Err(e) => {
                println!(": {}", "FAILED\n".red());
                eprint!("{e}");
            },
        }
    }

    println!("{}", "Uninstall succeed".green());
    Ok(())
}
