pub mod packet;
pub mod uart_client;
use anyhow::{Result, anyhow};
use serialport::SerialPort;
use std::{
    io,
    thread,
    time::{Duration, Instant},
};
use tracing::debug;

use crate::uart::packet::UARTPacket;

pub const SYNC_BYTES: [u8; 2] = [0xFF, 0x55];
pub const SYNC_LEN: usize = SYNC_BYTES.len();

pub struct Uart {
    _serial: Option<Box<dyn SerialPort>>,
    _buffer: Vec<u8>,

    _packet_timer: Instant,
    _waiting_packet: bool,

    _name: String,
    _baud_rate: u32,
}

impl Uart {
    /// Open uart port by name/path
    pub fn open(name: impl Into<String> + Clone, baud_rate: u32) -> Result<Self> {
        let serial = serialport::new(name.clone().into(), baud_rate)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| anyhow!("Cannot open serial port {}: {}", name.clone().into(), e))?;

        Ok(Self {
            _serial: Some(serial),
            _buffer: Vec::new(),

            _packet_timer: Instant::now(),
            _waiting_packet: false,
            _name: name.into(),
            _baud_rate: baud_rate,
        })
    }

    /// Trying toi extract next packet
    fn try_extract_packet(&mut self) -> Result<Option<UARTPacket>> {
        if self._packet_timer.elapsed() > Duration::from_millis(100)
            && self._buffer.len() >= SYNC_LEN
            && self._waiting_packet
        {
            self._buffer.remove(0);
            self._packet_timer = Instant::now();
            self._waiting_packet = false;
        }

        while self._buffer.len() >= SYNC_BYTES.len() && !self._buffer.starts_with(&SYNC_BYTES) {
            self._buffer.remove(0);
        }

        if self._buffer.len() < SYNC_LEN + 2 {
            return Ok(None);
        }

        if !self._waiting_packet {
            self._waiting_packet = true;
            self._packet_timer = Instant::now();
        }

        let packet_len =
            u16::from_le_bytes([self._buffer[SYNC_LEN], self._buffer[SYNC_LEN + 1]]) as usize;

        let total = SYNC_LEN + 2 + packet_len + 1;

        if self._buffer.len() < total {
            return Ok(None);
        }

        let packet: Vec<u8> = self._buffer.drain(..total).collect();
        self._waiting_packet = false;
        Ok(Some(UARTPacket::parse(packet)?))
    }

    /// Read and get first available packet
    ///
    /// If packet not available - returns Ok(None)
    pub fn read(&mut self) -> Result<Option<UARTPacket>, io::Error> {
        if let Some(serial) = &mut self._serial {
            let mut temp = [0u8; 64];

            match serial.read(&mut temp) {
                Ok(n) => {
                    self._buffer.extend_from_slice(&temp[..n]);
                    debug!("Received packet: {:?}", self._buffer);
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(e) => return Err(e.into()),
            }
        }

        self.try_extract_packet()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn send(&mut self, packet: UARTPacket) -> Result<usize> {
        if let Some(serial) = &mut self._serial {
            return Ok(serial.write(&packet.send())?);
        }

        Err(anyhow!("Serial not inited"))
    }

    /// Reinit UART connection on broken pipe
    pub fn reinit(&mut self) -> Result<()> {
        drop(self._serial.take());

        thread::sleep(Duration::from_millis(500));

        let serial = serialport::new(self._name.clone(), self._baud_rate)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| anyhow!("Cannot open serial port {}: {}", self._name, e))?;

        self._serial = Some(serial);

        Ok(())
    }
}

#[allow(unused)]
pub enum UARTCommand {
    Send(UARTPacket),
    Stop,
}
