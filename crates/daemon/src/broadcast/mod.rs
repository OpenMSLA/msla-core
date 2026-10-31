use anyhow::Result;
use msla_core::config;
use tokio::net::UdpSocket;
use tracing::{debug, error, info};

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
                let mut response: Vec<u8> = vec![0xAA, 0x55, 0x02];
                response.push(config.global.machine_name.len() as u8);
                response.extend_from_slice(config.global.machine_name.as_bytes());

                let ver = env!("CARGO_PKG_VERSION");
                response.push(ver.len() as u8);
                response.extend_from_slice(ver.as_bytes());

                socket.send_to(&response, addr).await?;
            },

            _ => {
                debug!("Unknown command");
            },
        }
    }
}
