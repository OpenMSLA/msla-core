use std::{net::IpAddr, thread};

use anyhow::{Result, anyhow};
use local_ip_address::local_ip;
use msla_core::{
    modify_config,
    types::printer_manager::{PrinterCommand, PrinterState},
};
use tokio::{
    runtime::Runtime,
    sync::{mpsc::Sender, watch::Receiver},
};
use tracing::{error, info};

use crate::{
    RUNTIME_CONFIG,
    control_display::{
        general_status::{build_general_status_response, build_model_info},
        preview_loader::get_preview_bytes,
        usb::{list_dir, load_fs_model},
    },
    peripheral::PeripheralController,
    uart::{Uart, packet::UARTPacket},
};

mod general_status;
mod preview_loader;
mod usb;

/// Create display thread
pub fn spawn_control_display_thread(
    mut uart: Uart,
    sender: Sender<PrinterCommand>,
    receiver: Receiver<PrinterState>,
    peripheral: PeripheralController,
) -> Result<()> {
    thread::spawn(move || {
        let tokio_rt = Runtime::new().unwrap();
        loop {
            match uart.read() {
                Ok(Some(packet)) => {
                    match handle_command(
                        &tokio_rt,
                        packet,
                        sender.clone(),
                        receiver.clone(),
                        peripheral.clone(),
                    ) {
                        Ok(Some(p)) => {
                            let _ = uart.send(p);
                        },
                        Err(e) => error!("Error at display request handle: {e}"),
                        _ => {},
                    }
                },
                Ok(None) => continue,
                Err(err) => {
                    error!("Failed to read from Uart: {:?}", err);
                    continue;
                },
            }
        }
    });

    Ok(())
}

fn handle_command(
    tokio_rt: &Runtime,
    mut packet: UARTPacket,
    sender: Sender<PrinterCommand>,
    state: Receiver<PrinterState>,
    peripheral: PeripheralController,
) -> Result<Option<UARTPacket>> {
    Ok(match packet.packet_id {
        // General status request
        10 => Some(build_general_status_response(tokio_rt, packet, state)),

        // Mechanical status request
        12 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);
            let ps = tokio_rt.block_on(peripheral.get_physical_state())?;

            p.write_u8(if ps.uv_state { 1 } else { 0 });
            p.write_f32(ps.z_pos as f32);

            Some(p)
        },

        // Machine info
        14 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            p.write_string(env!("CARGO_PKG_VERSION").to_owned());
            Some(p)
        },

        // Machine network status
        16 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            // Current local ip
            //      1 byte       4 bytes (octets) (u32) in le
            // <ip-availability> <ipv4>
            match local_ip() {
                Ok(ip) => match ip {
                    IpAddr::V4(ip) => {
                        p.write_u8(1);
                        p.write_u32(u32::from_le_bytes(ip.octets()));
                    },
                    // IDK why method that returns "local IPv4 address"
                    // returns IpAddr instead of Ipv4Addr
                    IpAddr::V6(_) => p.write_u8(0),
                },
                Err(e) => {
                    p.write_u8(0);
                    error!("Cannot get local ip: {e}")
                },
            }

            Some(p)
        },

        20 => {
            let side = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let count = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let offset = packet.read_u16().ok_or(anyhow!("can't parse"))?;

            let path = match state.borrow().clone() {
                PrinterState::Printing(st) | PrinterState::Paused(st) => {
                    st.model.model_preview.clone()
                },
                _ => anyhow::bail!("Wrong printer state"),
            };

            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            match get_preview_bytes(path, side, count, offset) {
                Ok(result) => {
                    p.write_u8(0); // SUCCESS
                    p.write_u16(offset / 2);
                    p.write_u16(result.len() as u16);
                    p.payload.extend(result);
                },
                Err(e) => {
                    error!("Preview load error: {e}");

                    p.write_u8(1); // ERROR
                },
            };

            Some(p)
        },

        // abort command
        40 => {
            let _ = sender.blocking_send(PrinterCommand::Abort);
            None
        },

        // Home command
        56 => {
            if !state.borrow().is_busy() {
                let _ = sender.blocking_send(PrinterCommand::Home);
            }
            None
        },

        // Home leveling move command
        60 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);
            if !state.borrow().is_busy() {
                let pos = packet.read_f64().ok_or(anyhow!("can't parse"))?;
                let _ = sender.blocking_send(PrinterCommand::MoveTo { pos, speed: 30.0 });

                p.write_u8(0);
                p.write_f32(pos as f32); // Return latest targeted pos
            } else {
                p.write_u8(1);
            }

            Some(p)
        },

        // Save home leveling
        62 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);
            if !state.borrow().is_busy() {
                let pos = packet.read_f64().ok_or(anyhow!("can't parse"))?;

                let res: anyhow::Result<()> = tokio_rt.block_on(async {
                    modify_config!(RUNTIME_CONFIG, leveling.bed_level_offset = pos);
                    RUNTIME_CONFIG.store_config().await?;

                    Ok(())
                });

                match res {
                    Ok(_) => {
                        info!("Config stored!");
                        p.write_u8(0);
                    },
                    Err(e) => {
                        error!("Cannot store config: {e}");
                        p.write_u8(1);
                        p.write_string(e.to_string());
                    },
                }
            } else {
                p.write_u8(1);
                p.write_string("Printer is busy");
            }

            Some(p)
        },

        // List dir
        70 => {
            let entry_id = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let page = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let take = packet.read_u16().ok_or(anyhow!("can't parse"))?;

            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            match tokio_rt.block_on(list_dir(entry_id, page, take)) {
                Ok(l) => p.extend(l),
                Err(e) => {
                    error!("Error during usb listing: {e}");
                    p.write_u16(0); // Entries len
                    p.write_u16(0); // Total len
                },
            }

            Some(p)
        },

        // Model info
        72 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);
            let file_id = packet.read_u16().ok_or(anyhow!("can't parse"))?;

            match tokio_rt.block_on(build_model_info(file_id)) {
                Ok(data) => {
                    p.write_u8(0); // Success
                    p.extend(data);
                },
                Err(e) => {
                    p.write_u8(1); // Error

                    error!("Cannot get model info print: {e}");
                    p.write_string(e.to_string());
                },
            }

            Some(p)
        },

        // Model file preview
        74 => {
            let file_id = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let side = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let count = packet.read_u16().ok_or(anyhow!("can't parse"))?;
            let offset = packet.read_u16().ok_or(anyhow!("can't parse"))?;

            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            match tokio_rt.block_on(load_fs_model(file_id)) {
                Ok(model) => {
                    match get_preview_bytes(model.model_preview.clone(), side, count, offset) {
                        Ok(result) => {
                            p.write_u8(0); // SUCCESS
                            p.write_u16(offset / 2);
                            p.write_u16(result.len() as u16);
                            p.payload.extend(result);
                        },
                        Err(e) => {
                            error!("Preview load error: {e}");

                            p.write_u8(1); // ERROR
                        },
                    }
                },
                Err(e) => {
                    error!("Model load error: {e}");

                    p.write_u8(1); // ERROR
                },
            }

            Some(p)
        },

        // Start usb print
        76 => {
            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            if state.borrow().is_busy() {
                p.write_u8(1);
                p.write_string("Cannot start print: Printer is busy");
                return Ok(Some(p));
            }

            let mut f = async || -> Result<()> {
                let file_id = packet.read_u16().ok_or(anyhow!("can't parse"))?;
                let model = load_fs_model(file_id).await?;

                sender.send(PrinterCommand::StartPrint(model)).await?;

                Ok(())
            };

            match tokio_rt.block_on(f()) {
                Ok(_) => p.write_u8(0), // Success
                Err(e) => {
                    p.write_u8(1); // Error

                    let e_str = e.to_string();
                    error!("Cannot start print: {}", e_str);
                    p.write_string(e_str);
                },
            }

            Some(p)
        },

        _ => None,
    })
}
