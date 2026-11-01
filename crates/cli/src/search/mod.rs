use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio::{net::UdpSocket, sync::mpsc::UnboundedSender, time::sleep};

use crate::search::data_reader::ByteReader;

mod data_reader;

const EXCLUDE_NAMES: [&str; 4] = ["vpn", "wsl", "loopback", "tun"];

fn find_local_network() -> Option<(Ipv4Addr, u32)> {
    let interfaces = if_addrs::get_if_addrs().ok()?;

    for iface in interfaces {
        if iface.is_loopback()
            || EXCLUDE_NAMES
                .iter()
                .any(|e| iface.name.to_lowercase().contains(e))
        {
            continue;
        }

        if let if_addrs::IfAddr::V4(v4) = iface.addr {
            let ip = v4.ip;
            let mask = v4.netmask;
            let mask_bits = u32::from(mask).count_ones();

            return Some((ip, mask_bits));
        }
    }
    None
}

fn find_broadcast() -> Option<IpAddr> {
    let interfaces = if_addrs::get_if_addrs().ok()?;

    for iface in interfaces {
        if iface.is_loopback()
            || EXCLUDE_NAMES
                .iter()
                .any(|e| iface.name.to_lowercase().contains(e))
        {
            continue;
        }
        if let if_addrs::IfAddr::V4(v4_addr) = iface.addr
            && let Some(broadcast) = v4_addr.broadcast
        {
            return Some(IpAddr::V4(broadcast));
        }
    }
    None
}

async fn send_discover(socket: &UdpSocket, alt: bool, port: u16) -> Result<()> {
    let packet = [
        0xAA, 0x55, 0x01, // DISCOVER
    ];

    if alt {
        // Unicast scan
        let (local_ip, prefix) = find_local_network().expect("Cannot find local network");

        if prefix > 30 {
            anyhow::bail!("Network is too small: /{prefix}");
        }

        let ip = u32::from(local_ip);

        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };

        let network = ip & mask;
        let broadcast = network | !mask;

        let first_host = network + 1;
        let last_host = broadcast - 1;

        for host in first_host..=last_host {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(host)), port);

            socket.send_to(&packet, addr).await?;
        }
    } else {
        // Broadcast
        let broadcast_ip = find_broadcast().expect("Cannot find broadcast address");

        socket.set_broadcast(true)?;

        socket
            .send_to(&packet, SocketAddr::new(broadcast_ip, port))
            .await?;
    }

    Ok(())
}

#[derive(Clone)]
pub struct FoundPrinter {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub ver: Option<String>,

    pub _last_updated: Instant,
}

/// Execute search operation
pub async fn start_search(
    alt: bool,
    port: u16,
    tx: UnboundedSender<FoundPrinter>,
) -> Result<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)> {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    socket.set_broadcast(true)?;

    let sender_socket = socket.clone();
    let sender_handle = tokio::spawn(async move {
        loop {
            let _ = send_discover(&sender_socket, alt, port).await;
            sleep(Duration::from_secs(5)).await;
        }
    });

    let listener_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            let received = socket.recv_from(&mut buf).await;
            if let Ok((size, addr)) = received {
                let mut reader = ByteReader::from(&buf, size);

                if !reader.validate_start_move() {
                    continue;
                }

                let name = reader.read_string();
                let ver = reader.read_string();

                let _ = tx.send(FoundPrinter {
                    ip: addr.ip(),
                    name,
                    ver,
                    _last_updated: Instant::now(),
                });
            }
        }
    });

    Ok((sender_handle, listener_handle))
}
