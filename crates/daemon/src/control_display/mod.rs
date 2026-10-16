use std::{net::IpAddr, thread};

use anyhow::{Result, anyhow};
use local_ip_address::local_ip;
use msla_core::types::printer_manager::{PrinterCommand, PrinterState};
use tokio::{
    runtime::Runtime,
    sync::{mpsc::Sender, watch::Receiver},
};
use tracing::error;

use crate::{
    control_display::{
        general_status::build_general_status_response,
        preview_loader::get_preview_bytes,
        usb::list_dir,
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
                PrinterState::Printing(st) | PrinterState::Paused(st) => st
                    .model
                    .model_preview
                    .clone()
                    .ok_or_else(|| anyhow!("Preview not available"))?,
                _ => anyhow::bail!("Wrong printer state"),
            };

            let mut p = UARTPacket::new_empty(packet.packet_id + 1);

            match get_preview_bytes(path, side, count, offset) {
                Ok(result) => {
                    p.write_u8(0); // SUCCESS
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

        _ => None,
    })
}
