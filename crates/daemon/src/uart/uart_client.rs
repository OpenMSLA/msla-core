use crate::uart::{UARTCommand, Uart, packet::UARTPacket};
use anyhow::{Result, anyhow};
use std::{
    collections::HashMap, io, sync::{
        Arc,
        OnceLock,
        mpsc::{self, Sender},
    }, thread::{self, sleep}, time::Duration,
};
use tokio::{
    sync::{Mutex, oneshot},
    time::timeout,
};
use tracing::{error, info, warn};

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub struct UARTClient {
    pub writer: OnceLock<Sender<UARTCommand>>,

    /// Waiting to answer
    pending: Mutex<HashMap<u8, oneshot::Sender<UARTPacket>>>,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
impl UARTClient {
    /// Start polling to provided channel and from provided uart
    pub fn spawn_uart(self: Arc<Self>, mut uart: Uart) -> Result<()> {
        let (sending_uarts_tx, sending_uarts_rx) = mpsc::channel::<UARTCommand>();
        self.clone()
            .writer
            .set(sending_uarts_tx)
            .map_err(|e| anyhow!("Cannot init Uart writer: {:?}", e))?;

        let runtime = tokio::runtime::Handle::current();

        thread::spawn(move || {
            loop {
                while let Ok(cmd) = sending_uarts_rx.try_recv() {
                    match cmd {
                        UARTCommand::Send(packet) => {
                            let _ = uart.send(packet);
                        },

                        UARTCommand::Stop => {
                            return;
                        },
                    }
                }

                match uart.read() {
                    Ok(Some(packet)) => {
                        let client = self.clone();
                        runtime.spawn(async move {
                            client.handle_packet(packet).await;
                        });
                    },
                    Ok(None) => continue,
                    Err(err) => {
                        error!("Failed to read from Uart: {:?}", err);

                        match err.kind() {
                            io::ErrorKind::BrokenPipe => {
                                info!("Detected broken pipe error! Trying to reinit serial...");
                                match uart.reinit() {
                                    Ok(_) => info!("Serial reinit success! Continue work..."),
                                    Err(e) => {
                                        error!("Could not reinit serial: {e}");
                                        sleep(Duration::from_secs(1));
                                    },
                                }
                            },

                            _ => {},
                        }
                        continue;
                    },
                }
            }
        });

        Ok(())
    }

    /// Create new UART client
    pub fn new(uart: Uart) -> Result<Arc<Self>> {
        let cl = Arc::new(Self {
            writer: OnceLock::new(),
            pending: Mutex::new(HashMap::default()),
        });

        cl.clone().spawn_uart(uart)?;

        Ok(cl)
    }

    /// Create new UART request
    pub async fn request(
        &self,
        packet: UARTPacket,
        response_timeout: Duration,
        retries: u8,
    ) -> Result<UARTPacket> {
        let response_id = packet.packet_id + 1;

        for _ in 0..=retries {
            let (tx, rx) = oneshot::channel();

            {
                let mut pending = self.pending.lock().await;

                if pending.contains_key(&response_id) {
                    anyhow::bail!(
                        "Uart request with response_id 0x{:02X} is already pending",
                        response_id
                    );
                }

                pending.insert(response_id, tx);
            }

            if let Err(err) = self.send(packet.clone()).await {
                self.pending.lock().await.remove(&response_id);
                return Err(err);
            }

            match timeout(response_timeout, rx).await {
                Ok(Ok(response)) => {
                    if response.packet_id != response_id {
                        continue;
                    }
                    return Ok(response);
                },

                Ok(Err(_)) => {
                    self.pending.lock().await.remove(&response_id);
                    anyhow::bail!("Uart reader stopped");
                },

                Err(_) => {
                    warn!("Uart: timeout waiting for 0x{:02X}", response_id);
                    self.pending.lock().await.remove(&response_id);
                },
            }
        }

        anyhow::bail!("Uart request timeout after {} retries", retries)
    }

    /// Send packet to uart
    pub async fn send(&self, packet: UARTPacket) -> Result<()> {
        let w = self.writer.get();
        let writer = w
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Uart writer is not initialized"))?;

        writer.send(UARTCommand::Send(packet.clone()))?;
        Ok(())
    }

    /// Receive the package and send it
    async fn handle_packet(&self, packet: UARTPacket) {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(&packet.packet_id)
        };

        if let Some(tx) = tx {
            let _ = tx.send(packet);
        } else {
            warn!(
                "Uart: Received unexpected packet 0x{:02X}",
                packet.packet_id
            );
        }
    }

    /// Resets all pending mutexes
    pub async fn reset_client(&self) {
        self.pending.lock().await.clear();
    }
}
