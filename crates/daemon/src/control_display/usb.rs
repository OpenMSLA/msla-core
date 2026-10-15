use crate::{uart::packet::UARTPacket, usb_service::fs_service::UsbFilesystem};
use anyhow::{Result, anyhow};
use tracing::{debug, info};

/// Returns a requested dir list
pub async fn list_dir(id: u16, page: u16, take: u16) -> Result<UARTPacket> {
    debug!("Requested fs with id {id}, page {page}");

    let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;

    if !fs.read().await.available {
        anyhow::bail!("Fs not available");
    }

    let (parent_id, list_dir) = fs.write().await.traverse_dir(id).await?;

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
    packet.write_u16(parent_id);
    packet.write_u16(items.len() as u16);

    items
        .iter()
        .for_each(|k| list_dir.get(k).unwrap().serialize_to(&mut packet));

    packet.write_u16(keys.len() as u16); // Total entries

    info!("File list packet size: {} bytes", packet.payload.len());

    Ok(packet)
}
