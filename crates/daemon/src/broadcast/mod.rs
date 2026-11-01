use anyhow::Result;
use msla_core::{SUPPORTED_FILE_EXTENSIONS, config};
use tokio::net::UdpSocket;
use tracing::{debug, error, info};

use crate::broadcast::data_writer::DataWriter;
mod data_writer;

/// Start broadcast listener
pub async fn start_broadcast() {
    tokio::select! {
        res = broadcast() => {
            if let Err(e) = res {
                error!("Broadcast listener err: {}", e);
            }
        },
        _ = tokio::signal::ctrl_c() => {
            info!("Stopping broadcast server");
        }
    }
}

async fn broadcast() -> Result<()> {
    let config = config::get_config().await;
    let socket = UdpSocket::bind((
        config.broadcast_listener.addr.clone(),
        config.broadcast_listener.port,
    ))
    .await?;

    info!("Started UDP broadcast server");

    let mut buf = [0u8; 1500];
    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        let packet = &buf[..len];

        if packet.len() < 3 {
            continue;
        }

        if packet[0] != 0xAA || packet[1] != 0x55 {
            continue;
        }

        match packet[2] {
            // DISCOVER
            0x01 => {
                debug!("Discovery from {addr}");

                let mut dw = DataWriter::new();
                dw.payload.extend(&[0xAA, 0x55, 0x02]);

                dw.write_string(&config.global.machine_name);
                dw.write_string(env!("CARGO_PKG_VERSION"));

                dw.write_u8(SUPPORTED_FILE_EXTENSIONS.len() as u8);
                for ext in SUPPORTED_FILE_EXTENSIONS {
                    dw.write_string(ext);
                }

                socket.send_to(dw.send(), addr).await?;
            },

            _ => {
                debug!("Unknown command");
            },
        }
    }
}
