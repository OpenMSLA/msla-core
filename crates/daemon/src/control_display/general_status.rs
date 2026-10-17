use anyhow::{Result, anyhow};

use msla_core::types::printer_manager::PrinterState;
use tokio::{runtime::Runtime, sync::watch::Receiver};

use crate::{
    control_display::usb::load_fs_model,
    uart::packet::UARTPacket,
    usb_service::fs_service::UsbFilesystem,
};

pub fn build_general_status_response(
    tokio_rt: &Runtime,
    packet: UARTPacket,
    state: Receiver<PrinterState>,
) -> UARTPacket {
    let mut p = UARTPacket::new_empty(packet.packet_id + 1);
    let st = state.borrow().clone();

    let fs = tokio_rt.block_on(async {
        let Some(fs) = UsbFilesystem::get_filesystem() else {
            return false;
        };

        fs.read().await.available
    });

    // Flags array
    let flags: u8 = fs as u8; // Usb availability
    // flags |= (true as u8) << 1;

    // Flags
    p.write_u8(flags);

    match &st {
        PrinterState::Idle => p.write_u8(0),
        PrinterState::Printing(s) | PrinterState::Paused(s) => {
            let current_ir = s.model.ir.get(s.current_ir_index);
            let est = current_ir
                .map(|ir| ir.estimated_remaining)
                .unwrap_or_default();

            let current_ir_duration = current_ir
                .map(|ir| ir.calc_command_duration())
                .unwrap_or_default();

            match st {
                PrinterState::Printing(_) => p.write_u8(1),
                PrinterState::Paused(_) => p.write_u8(2),
                _ => unreachable!(),
            }

            // LAYERS
            // current layer
            p.write_u16(s.printing_layer as u16);
            // total layers
            p.write_u16(s.model.model_meta.total_layer_count as u16);

            // IR
            // current ir index
            p.write_u32(s.current_ir_index as u32);
            // total ir len
            p.write_u32(s.model.ir.len() as u32);

            // IR DURATION
            // duration of current ir
            p.write_u32(current_ir_duration.as_secs_f32() as u32);
            // full estimated printing time
            p.write_u32(s.model.model_meta.estimated_printing_time as u32);
            // estimated finish time
            p.write_u32(
                est.checked_sub(s.current_ir_elapsed.elapsed())
                    .unwrap_or_default()
                    .as_secs() as u32,
            );

            // total elapsed secs
            p.write_u32(s.total_elapsed.elapsed().as_secs() as u32);
        },
        PrinterState::Error(e) => {
            p.write_u8(3);
            p.write_string(e.to_string());
        },
        PrinterState::Busy => p.write_u8(4),
        PrinterState::Aborted => p.write_u8(5),
        PrinterState::Finished { total_elapsed: te } => {
            p.write_u8(6);
            p.write_u32(te.elapsed().as_secs() as u32);
        },
    };

    p
}

/// Build model info packet
pub async fn build_model_info(model_id: u16) -> Result<UARTPacket> {
    let mut p = UARTPacket::new_empty(0);
    let model = load_fs_model(model_id).await?;

    let file_size = {
        let fs = UsbFilesystem::get_filesystem().ok_or(anyhow!("FS not available"))?;
        fs.read()
            .await
            .file_index
            .get(&model_id)
            .ok_or(anyhow!("Cannot load model: File not found"))
            .map(|e| e.file_size)
    }?;

    // total layers
    p.write_u16(model.model_meta.total_layer_count as u16);

    // full estimated printing time
    p.write_u32(model.model_meta.estimated_printing_time as u32);

    // Layer height
    p.write_f32(model.model_meta.layer_height.unwrap_or_default() as f32);

    // File size in bytes
    p.write_u32(file_size);

    if let Some(ref name) = model.model_meta.file_name {
        p.write_u8(1);
        p.write_string(name.to_string());
    } else {
        p.write_u8(0); // No name
    }

    Ok(p)
}
