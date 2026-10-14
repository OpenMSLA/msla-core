use anyhow::Result;
use msla_core::config;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs, process::Command, time::sleep};
use tracing::{error, info, warn};

use crate::usb_service::fs_service::UsbFilesystem;

pub mod fs_service;

/// Gets all device list, matching provided regex
async fn get_device_list(path: &PathBuf, re: &Regex) -> Result<HashSet<PathBuf>> {
    if !path.exists() {
        anyhow::bail!("Path {:?} not exists", path);
    }

    if !path.is_dir() {
        anyhow::bail!("Path {:?} not a directory", path);
    }

    let mut found_hashmap = HashSet::new();
    let mut read_dir = fs::read_dir(&path).await?;
    while let Some(file) = read_dir.next_entry().await? {
        let name = file.file_name().to_string_lossy().into_owned();

        if re.is_match(&name) {
            found_hashmap.insert(file.path().canonicalize()?);
        }
    }

    Ok(found_hashmap)
}

/// Checks if the provided folder is already mounted
async fn is_mount_folder(folder: &PathBuf) -> Option<PathBuf> {
    let res = Command::new("findmnt")
        .arg("-no")
        .arg("SOURCE")
        .arg(folder)
        .output()
        .await;

    match res {
        Ok(o) if o.status.success() => {
            let output = String::from_utf8_lossy(&o.stdout).into_owned();
            let dev = output.trim();
            (!dev.is_empty()).then(|| PathBuf::from(dev))
        },
        Err(e) => {
            error!("Cannot check mountpoint: {e}");
            None
        },

        _ => None,
    }
}

/// Finds first available mount folder
async fn get_free_mount_folder(
    part_table: &HashMap<PathBuf, PathBuf>,
    mount_path: &Path,
) -> PathBuf {
    let mut x = 0;
    let values: Vec<PathBuf> = part_table.values().cloned().collect();

    loop {
        let s = mount_path.join(format!("part{x}"));
        if !values.contains(&s) && is_mount_folder(&s).await.is_none() {
            return s;
        }

        x += 1;
    }
}

/// Calls `blkid` command and returns true, if there is a fs
async fn check_fs_existence(dev: &PathBuf) -> bool {
    let res = Command::new("blkid").arg(dev).output().await;

    let output = match res {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            error!(
                "Cannot check fs existence: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            return false;
        },
        Err(e) => {
            error!("Cannot check fs existence: {e}");
            return false;
        },
    };

    let iter = output.split_whitespace();
    for part in iter {
        if part.starts_with("TYPE") {
            return true;
        }
    }

    false
}

/// Mount device to a folder
async fn mount_device(dev: &PathBuf, path: &PathBuf) -> Result<()> {
    // If mount path not exists - create
    if !path.exists() {
        fs::create_dir_all(&path).await?;
    }

    let res = Command::new("mount").arg(dev).arg(path).output().await?;

    if !res.status.success() {
        anyhow::bail!(
            "Could not mount device: {}",
            String::from_utf8_lossy(&res.stderr)
        )
    }

    Ok(())
}

/// Unmount device
async fn unmount_device(path: &PathBuf) {
    let res = Command::new("umount").arg(path).output().await;
    match res {
        Ok(o) => {
            if !o.status.success() {
                error!(
                    "Could not unmount device: {}",
                    String::from_utf8_lossy(&o.stderr)
                )
            }
        },
        Err(e) => error!("Error calling umount: {e}"),
    }

    // removes old dir
    if let Err(e) = fs::remove_dir_all(path).await {
        error!("Could not delete old mount dir: {e}")
    }
}

/// Starts usb mounter service loop
pub async fn start_usb_service() -> Result<()> {
    let mut dirty = false;
    let config = config::get_config().await;
    let re = Regex::new(&config.usb_controller.usb_path_regex)?;

    let scan_duration = Duration::from_secs(config.usb_controller.scan_period as u64);

    // dev path-to-mount folder table
    // e.g. (/dev/sda1 -> part0)
    let mut partitions_table: HashMap<PathBuf, PathBuf> = HashMap::new();

    // Detected devices at last check
    let mut last_connected_devices: HashSet<PathBuf> = HashSet::new();

    // Init usb fs with default path
    UsbFilesystem::init(config.usb_controller.mount_point.clone())?;

    let mut already_mounted = fs::read_dir(&config.usb_controller.mount_point).await?;
    while let Some(dir) = already_mounted.next_entry().await? {
        if let Some(dev) = is_mount_folder(&dir.path()).await {
            info!(
                "Found already mounted device: {:?} -> {:?}",
                dev,
                dir.path()
            );

            dirty = true;
            partitions_table.insert(dev.clone(), dir.path());
            last_connected_devices.insert(dev);
        } else {
            match fs::remove_dir_all(dir.path()).await {
                Ok(()) => info!("Removed unused mount folder: {:?}", dir.path()),
                Err(e) => error!("Cannot delete mount folder: {e}"),
            }
        }
    }

    loop {
        let found_dev = match get_device_list(&config.usb_controller.scan_path, &re).await {
            Ok(d) => d,
            Err(e) => {
                error!("Cannot scan device list: {e}");
                sleep(scan_duration).await;

                continue;
            },
        };

        for added in found_dev.difference(&last_connected_devices) {
            info!("Added device: {:?}", added);

            if !check_fs_existence(added).await {
                warn!("Filesystem on device {:?} not found! Skipping it...", added);
                continue;
            }

            let free_mount_folder =
                get_free_mount_folder(&partitions_table, &config.usb_controller.mount_point).await;

            match mount_device(added, &free_mount_folder).await {
                Ok(()) => {
                    info!(
                        "Mounted new usb device: {:?} -> {:?}",
                        added, free_mount_folder
                    );

                    partitions_table.insert(added.to_path_buf(), free_mount_folder);
                    dirty = true;
                },
                Err(e) => {
                    error!("Cannot mount device {:?}: {e}", added)
                },
            }
        }

        for removed in last_connected_devices.difference(&found_dev) {
            info!("Removed device: {:?}", removed);

            match partitions_table.get(removed) {
                Some(path) => {
                    unmount_device(path).await;
                    partitions_table.remove(removed);
                    dirty = true;
                },
                None => warn!("Device {:?} not mounted", removed),
            }
        }

        if dirty {
            let devices_len = found_dev.len();
            let root_dir = if devices_len == 1 {
                found_dev
                    .iter()
                    .next()
                    .and_then(|x| partitions_table.get(x).cloned())
                    .unwrap_or_else(|| config.usb_controller.mount_point.clone())
            } else {
                config.usb_controller.mount_point.clone()
            };

            if let Some(fs) = UsbFilesystem::get_filesystem() {
                let mut write_guard = fs.write().await;
                write_guard.reinit(root_dir);
                write_guard.set_available(!partitions_table.is_empty());
            }

            dirty = false;
        }

        last_connected_devices = found_dev;
        sleep(scan_duration).await;
    }
}
