use crate::{uart::packet::UARTPacket, usb_service::fs_service::UsbFilesystem};
use anyhow::{Result, anyhow};
use tracing::info;

/// Returns a requested dir list
pub async fn list_dir(id: u16, page: u16, take: u16) -> Result<UARTPacket> {
    let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;

    if !fs.read().await.available {
        anyhow::bail!("Fs not available");
    }

    let list_dir = fs.write().await.traverse_dir(id).await?;

    let mut keys: Vec<_> = list_dir.keys().collect();
    keys.sort();

    let start = (page * take) as usize;
    let end = (start + take as usize).min(keys.len());

    let items = if start < keys.len() {
        &keys[start..end]
    } else {
        &keys[0..0]
    };

    let mut packet = UARTPacket::new_empty(0);
    packet.write_u16(items.len() as u16);

    for key in items {
        let el = list_dir.get(key).unwrap();

        packet.write_u16(el.id); // Entry id
        packet.write_u16(el.parent_id); // Parent id
        packet.write_u8(el.file_type.clone() as u8); // File type
        packet.write_string(el.file_name.clone()); // File name
    }

    packet.write_u16(keys.len() as u16); // Total entries

    info!("File list packet size: {} bytes", packet.payload.len());

    Ok(packet)
}
