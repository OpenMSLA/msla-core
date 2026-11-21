use anyhow::Result;
use std::path::PathBuf;

#[cfg(windows)]
pub async fn add_to_path(path: PathBuf) -> Result<()> {
    use anyhow::anyhow;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST,
        SMTO_ABORTIFHUNG,
        SendMessageTimeoutW,
        WM_SETTINGCHANGE,
    };

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey("Environment")?;

    let current: String = key.get_value("Path").unwrap_or_default();
    let mut paths = std::env::split_paths(&current).collect::<Vec<PathBuf>>();

    if paths.iter().any(|k| {
        k.to_string_lossy()
            .eq_ignore_ascii_case(&path.to_string_lossy())
    }) {
        return Ok(());
    }

    paths.push(path);
    let new_path = std::env::join_paths(paths).map_err(|e| anyhow!("Cannot join new path: {e}"))?;

    key.set_value("Path", &new_path.to_string_lossy().to_string())?;

    // Notify win about new path (f*king windows)
    let environment: Vec<u16> = "Environment".encode_utf16().chain(Some(0)).collect();
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            std::ptr::null_mut(),
        );
    }

    Ok(())
}

#[cfg(windows)]
pub async fn remove_from_path(path: PathBuf) -> Result<()> {
    use anyhow::anyhow;
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST,
        SMTO_ABORTIFHUNG,
        SendMessageTimeoutW,
        WM_SETTINGCHANGE,
    };

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(
        "Environment",
        winreg::enums::KEY_READ | winreg::enums::KEY_WRITE,
    )?;

    let current: String = key.get_value("Path").unwrap_or_default();

    let paths: Vec<_> = std::env::split_paths(&current)
        .filter(|p| {
            !p.to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
        })
        .collect();

    if paths.len() == std::env::split_paths(&current).count() {
        return Ok(());
    }

    let new_path = std::env::join_paths(paths).map_err(|e| anyhow!("{}", e))?;
    key.set_value("Path", &new_path.to_string_lossy().to_string())?;

    let environment: Vec<u16> = "Environment".encode_utf16().chain(Some(0)).collect();
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            std::ptr::null_mut(),
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn add_to_path(path: PathBuf) -> Result<()> {
    use anyhow::anyhow;
    use tokio::fs;

    let home = std::env::var_os("HOME").ok_or(anyhow!("Cannot find HOME dir"))?;
    let profile = PathBuf::from(home).join(".profile");

    let mut content = fs::read_to_string(&profile).await.unwrap_or_default();
    let path_str = path.to_string_lossy();

    if content.contains(&format!("'{}'", path_str)) {
        return Ok(());
    }

    let escaped = path_str.replace("'", "'\\''");

    content.push_str(&format!(
        "\n# Added by OpenMSLA CLI\nexport PATH=\"$PATH\":'{}'\n",
        escaped
    ));

    fs::write(profile, content).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn remove_from_path(path: &PathBuf) -> Result<()> {
    use anyhow::anyhow;
    use tokio::fs;

    let home = std::env::var_os("HOME").ok_or(anyhow!("Cannot find HOME dir"))?;
    let profile = PathBuf::from(home).join(".profile");

    let content = fs::read_to_string(&profile).await.unwrap_or_default();
    let marker = format!("export PATH=\"$PATH\":'{}'", path.to_string_lossy());

    let new_content = content
        .lines()
        .filter(|line| *line != marker)
        .collect::<Vec<_>>()
        .join("\n");

    if new_content == content {
        return Ok(());
    }

    fs::write(profile, new_content).await?;

    Ok(())
}
