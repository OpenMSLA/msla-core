use crate::{uart::packet::UARTPacket, usb_service::fs_service::UsbFilesystem};
use anyhow::{Result, anyhow};

/// Returns a requested dir list 
pub async fn list_dir(id: u16) -> Result<UARTPacket> {
    let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;

    if !fs.read().await.available {
        anyhow::bail!("Fs not available");
    }

    let list_dir = fs.write().await.traverse_dir(id).await?;
    println!("{:?}", list_dir);

    Ok(UARTPacket::new_empty(0))
}
