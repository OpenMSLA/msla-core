use std::sync::{Arc, OnceLock};

use crate::{uart::packet::UARTPacket, usb_service::fs_service::UsbFilesystem};
use anyhow::{Result, anyhow};
use msla_core::{
    model_parser::{photon::load_photon_model, zip::load_zip_model},
    types::model::Model,
};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Returns a requested dir list
pub async fn list_dir(id: u16, page: u16, take: u16) -> Result<UARTPacket> {
    debug!("Requested fs with id {id}, page {page}");

    let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;

    if !fs.read().await.available {
        anyhow::bail!("Fs not available");
    }

    let (parent_id, list_dir) = fs.write().await.dir_contents(id).await?;

    let mut keys: Vec<_> = list_dir.iter().collect();
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

    {
        let fs_read_handle = fs.read().await;
        items.iter().for_each(|k| {
            fs_read_handle
                .file_index
                .get(k)
                .unwrap()
                .serialize_to(&mut packet)
        });
    }

    packet.write_u16(keys.len() as u16); // Total entries

    info!("File list packet size: {} bytes", packet.payload.len());

    Ok(packet)
}

type CachedModelType = OnceLock<RwLock<Option<(u16, Arc<Model>)>>>;
static CACHED_MODEL: CachedModelType = OnceLock::new();

/// Load model from fs
pub async fn load_fs_model(file_id: u16) -> Result<Arc<Model>> {
    {
        let cm = CACHED_MODEL.get_or_init(|| None.into());
        let guard = cm.read().await;

        if let Some(g) = &*guard
            && g.0 == file_id
        {
            return Ok(g.1.clone());
        }
    }

    let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;
    let model_file = fs
        .read()
        .await
        .file_index
        .get(&file_id)
        .ok_or(anyhow!("Model not found"))?
        .path
        .clone();

    let model = match model_file.extension().and_then(|ext| ext.to_str()) {
        Some("zip") => load_zip_model(model_file).await?,
        Some("photon") => load_photon_model(model_file).await?,
        _ => anyhow::bail!("Invalid file ext"),
    };

    // Save loaded model into cache
    *CACHED_MODEL.get().unwrap().write().await = Some((file_id, model.clone()));

    Ok(model)
}
