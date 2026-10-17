use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    config,
    types::{
        model::{
            GlobalPrintingMeta,
            Model,
            ir::{PrintingIR, ZMoving},
        },
        model_parser::photon::{PhotonFileHeader, PhotonFileLayer},
    },
};
use anyhow::{Result, anyhow};
use image::{ImageBuffer, Luma};
use tempfile::tempdir;
use tokio::fs;

const HEADER_SIZE: usize = 108;
const LAYER_DEF_SIZE: usize = 36;

/// Read unsigned 32 at provided offset
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(anyhow!("Unexpected EOF"))?;

    Ok(u32::from_le_bytes(bytes.try_into()?))
}

/// Read signed 32 at provided offset
fn i32_at(data: &[u8], offset: usize) -> Result<i32> {
    Ok(u32_at(data, offset)? as i32)
}

/// Read float 32 at provided offset
fn f32_at(data: &[u8], offset: usize) -> Result<f32> {
    Ok(f32::from_bits(u32_at(data, offset)?))
}

/// Read .photon file header
fn read_header(data: &[u8]) -> Result<PhotonFileHeader> {
    if data.len() < HEADER_SIZE {
        anyhow::bail!("file is smaller than Photon header");
    }

    Ok(PhotonFileHeader {
        bed_x: f32_at(data, 0x08)?,
        bed_y: f32_at(data, 0x0C)?,
        bed_z: f32_at(data, 0x10)?,

        layer_height: f32_at(data, 0x20)?,
        exposure: f32_at(data, 0x24)?,
        bottom_exposure: f32_at(data, 0x28)?,
        off_time: f32_at(data, 0x2C)?,

        bottom_layers: i32_at(data, 0x30)?,
        width: i32_at(data, 0x34)?,
        height: i32_at(data, 0x38)?,

        preview_high: i32_at(data, 0x3C)?,
        layer_def_address: i32_at(data, 0x40)?,
        layer_count: i32_at(data, 0x44)?,
        preview_low: i32_at(data, 0x48)?,

        projection_type: i32_at(data, 0x50)?,

        bottom_lift_distance: f32_at(data, 0x70)?,
        bottom_lift_speed: f32_at(data, 0x74)?,
        lifting_distance: f32_at(data, 0x78)?,
        lifting_speed: f32_at(data, 0x7C)?,
        retract_speed: f32_at(data, 0x80)?,
    })
}

/// Read contained layer files
fn read_layer_defs(data: &[u8], header: &PhotonFileHeader) -> Result<Vec<PhotonFileLayer>> {
    let start = usize::try_from(header.layer_def_address)?;
    let count = usize::try_from(header.layer_count)?;

    let size = count
        .checked_mul(LAYER_DEF_SIZE)
        .ok_or(anyhow!("layer table too large"))?;

    let end = start
        .checked_add(size)
        .ok_or(anyhow!("layer table overflow"))?;

    if end > data.len() {
        anyhow::bail!(
            "layer table outside file: 0x{start:X}..0x{end:X}, file size 0x{:X}",
            data.len()
        );
    }

    let mut layers = Vec::with_capacity(count);

    for i in 0..count {
        let off = start + i * LAYER_DEF_SIZE;

        layers.push(PhotonFileLayer {
            height: f32_at(data, off)?,
            exposure: f32_at(data, off + 4)?,
            off_time: f32_at(data, off + 8)?,
            data_address: i32_at(data, off + 12)?,
            data_length: i32_at(data, off + 16)?,
        });
    }

    Ok(layers)
}

/// Decodes photon rle into byte vector
fn decode_rle(encoded: &[u8], pixel_count: usize) -> Result<Vec<u8>> {
    let mut pixels = Vec::with_capacity(pixel_count);

    for &byte in encoded {
        let value = if byte & 0x80 != 0 { 255 } else { 0 };
        let count = (byte & 0x7F) as usize;

        if count == 0 {
            anyhow::bail!("RLE record has zero length");
        }

        if pixels.len() + count > pixel_count {
            anyhow::bail!(
                "RLE overflow: decoded {}, expected {}",
                pixels.len() + count,
                pixel_count
            );
        }

        pixels.resize(pixels.len() + count, value);
    }

    if pixels.len() != pixel_count {
        anyhow::bail!(
            "RLE ended at {} pixels, expected {}",
            pixels.len(),
            pixel_count
        );
    }

    Ok(pixels)
}

/// saves png into provided path
fn save_png(pixels: &[u8], width: u32, height: u32, path: &Path) -> Result<()> {
    let image = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(width, height, pixels.to_vec())
        .ok_or(anyhow!("invalid image dimensions"))?;

    image.save(path)?;
    Ok(())
}

/// Extract preview image
fn extract_preview(data: &[u8], address: usize, output: impl AsRef<std::path::Path>) -> Result<()> {
    let width = i32_at(data, address)? as usize;
    let height = i32_at(data, address + 4)? as usize;
    let image_address = i32_at(data, address + 8)? as usize;
    let data_length = i32_at(data, address + 12)? as usize;

    let image_data = data
        .get(image_address..image_address + data_length)
        .ok_or(anyhow!("preview image data is out of bounds"))?;

    let pixel_count = width * height;
    let mut pixels = Vec::with_capacity(pixel_count);

    let mut pos = 0;

    while pos + 2 <= image_data.len() && pixels.len() < pixel_count {
        let value = u16::from_le_bytes([image_data[pos], image_data[pos + 1]]);
        pos += 2;

        let r = ((value >> 11) & 0x1F) as u8;
        let g = ((value >> 6) & 0x1F) as u8;
        let b = (value & 0x1F) as u8;

        let r = (r << 3) | (r >> 2);
        let g = (g << 3) | (g >> 2);
        let b = (b << 3) | (b >> 2);

        // This pixel is always present once.
        pixels.push([r, g, b]);

        if value & 0x20 != 0 {
            if pos + 2 > image_data.len() {
                anyhow::bail!("truncated preview RLE data");
            }

            let repeat = u16::from_le_bytes([image_data[pos], image_data[pos + 1]]) & 0x0FFF;
            pos += 2;

            for _ in 0..repeat {
                if pixels.len() >= pixel_count {
                    break;
                }

                pixels.push([r, g, b]);
            }
        }
    }

    if pixels.len() != pixel_count {
        anyhow::bail!(
            "invalid preview: decoded {} pixels, expected {}",
            pixels.len(),
            pixel_count
        );
    }

    let mut raw = Vec::with_capacity(pixel_count * 3);

    for [r, g, b] in pixels {
        raw.extend_from_slice(&[r, g, b]);
    }

    let image = image::RgbImage::from_raw(width as u32, height as u32, raw)
        .ok_or(anyhow!("failed to create preview image"))?;

    image.save(output)?;
    Ok(())
}

/// Load .photon file
pub async fn load_photon_model(photon_path: impl AsRef<std::path::Path>) -> Result<Arc<Model>> {
    let temp_dir = tempdir().map_err(|e| anyhow!("Cannot create temp dir: {}", e))?;
    let data = fs::read(&photon_path).await?;

    let header = read_header(&data)?;
    let layers = read_layer_defs(&data, &header)?;

    let width = u32::try_from(header.width)?;
    let height = u32::try_from(header.height)?;
    let pixel_count = width as usize * height as usize;

    let mut command_vec: Vec<PrintingIR> = Vec::new();

    // Turn UV off
    command_vec.push(PrintingIR::TurnUV(false));

    // Home printer
    command_vec.push(PrintingIR::Home);

    for (i, layer) in layers.iter().enumerate() {
        let start = usize::try_from(layer.data_address)?;
        let length = usize::try_from(layer.data_length)?;
        let end = start
            .checked_add(length)
            .ok_or(anyhow!("layer data offset overflow"))?;

        if end > data.len() {
            anyhow::bail!("layer {i} data outside file: 0x{start:X}..0x{end:X}");
        }

        let encoded = &data[start..end];
        let pixels = decode_rle(encoded, pixel_count)?;

        // Save layer image into temp dir
        let filename = format!("{i}.png");
        let output = temp_dir.path().join(&filename);
        save_png(&pixels, width, height, &output)?;

        // ------ Display layer ------
        command_vec.push(PrintingIR::ShowImage(PathBuf::from(filename)));

        // --------- Lift z ----------
        let (lift_distance, lift_speed) = if i as i32 <= header.bottom_layers {
            (header.bottom_lift_distance, header.bottom_lift_speed)
        } else {
            (header.lifting_distance, header.lifting_speed)
        };

        command_vec.push(PrintingIR::MoveZ(ZMoving::new(
            (layer.height + lift_distance) as f64,
            lift_speed as f64,
        )));

        // --- Move z to layer height ---
        command_vec.push(PrintingIR::MoveZ(ZMoving::new(
            layer.height as f64,
            header.retract_speed as f64,
        )));

        // ------- Turn UV on --------
        command_vec.push(PrintingIR::TurnUV(true));

        // ---------- Wait... --------
        command_vec.push(PrintingIR::Wait(Duration::from_secs_f32(layer.exposure)));

        // ------- Turn UV off --------
        command_vec.push(PrintingIR::TurnUV(false));
    }

    // Print-end code
    // Turn off UV
    command_vec.push(PrintingIR::TurnUV(false));

    // Slowly raise Z a little
    let last_layer_height = layers.last().map(|l| l.height).unwrap_or_default();
    command_vec.push(PrintingIR::MoveZ(ZMoving::new(
        (last_layer_height + 5.0) as f64,
        30.0,
    )));

    // Fast raise Z to the end
    // Note: Peripheral controller constrains the lifting height
    let config = config::get_config().await;
    command_vec.push(PrintingIR::MoveZ(ZMoving::new(
        config.physical.machine_height as f64,
        300.0,
    )));

    // And, finally, disable stepper
    command_vec.push(PrintingIR::DisableSteppers);

    let try_preview_path = temp_dir.path().join("preview.jpg");
    let preview_path = match extract_preview(&data, header.preview_high as usize, &try_preview_path)
    {
        Ok(_) => Some(try_preview_path),
        Err(_) => None,
    };

    // Construct meta struct
    let print_meta = GlobalPrintingMeta {
        file_name: photon_path
            .as_ref()
            .file_name()
            .map(|s| s.to_string_lossy().into_owned()),
        total_layer_count: layers.len(),
        estimated_printing_time: 0,
        volume: None,
        weight: None,
        price: None,
        layer_height: Some(header.layer_height),
    };

    Ok(Arc::new(Model::new(
        command_vec.iter().map(PrintingIR::to_timed_ir).collect(),
        print_meta,
        Arc::new(temp_dir),
        preview_path,
    )))
}
