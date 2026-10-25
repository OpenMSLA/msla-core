use crate::uart::{SYNC_BYTES, SYNC_LEN};
use anyhow::{Result, anyhow};
use tracing::debug;

/// CRC8 Generator
fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0x00;

    for byte in data {
        crc ^= byte;

        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }

    crc
}

#[derive(Debug, Clone)]
pub struct UARTPacket {
    /// Packet id (cmd)
    pub packet_id: u8,

    /// Payload of packet
    pub payload: Vec<u8>,

    _reader_pos: usize,
}

impl UARTPacket {
    pub fn new(id: u8, payload: &[u8]) -> Self {
        Self {
            packet_id: id,
            payload: payload.to_vec(),
            _reader_pos: 0,
        }
    }

    pub fn new_empty(id: u8) -> Self {
        Self {
            packet_id: id,
            payload: Vec::new(),
            _reader_pos: 0,
        }
    }

    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let payload_len_bytes = data
            .get(SYNC_LEN..SYNC_LEN + 2)
            .ok_or(anyhow!("Can't extract payload length"))?;
        let payload_len = u16::from_le_bytes([payload_len_bytes[0], payload_len_bytes[1]]) as usize;

        if payload_len < 1 {
            return Err(anyhow!("Can't parse packet: Payload is too short"));
        }

        let packet_end = SYNC_LEN
            .checked_add(2)
            .and_then(|offset| offset.checked_add(payload_len))
            .ok_or(anyhow!("Packet length overflow"))?;
        let received_crc = *data
            .get(packet_end)
            .ok_or(anyhow!("Cannot extract packet CRC"))?;

        let expected_crc = crc8(
            data.get(SYNC_LEN + 2..packet_end)
                .ok_or(anyhow!("Not enough data for packet payload"))?,
        );

        if received_crc != expected_crc {
            return Err(anyhow!(
                "Invalid packet CRC: expected 0x{expected_crc:02X}, got 0x{received_crc:02X}"
            ));
        }

        let cmd = data
            .get(SYNC_LEN + 2)
            .ok_or(anyhow!("Cannot extract cmd bytes from packet"))?;

        let payload_start = SYNC_LEN + 3; // LEN + CMD = 3
        let payload = data
            .get(payload_start..payload_start + (payload_len - 1))
            .ok_or(anyhow!("Not enough data in payload"))?;

        Ok(Self {
            packet_id: *cmd,
            payload: payload.to_vec(),
            _reader_pos: 0,
        })
    }

    /// Build packet for sending
    pub fn send(&self) -> Vec<u8> {
        let mut buff: Vec<u8> = Vec::new();

        let mut finish_payload = self.payload.clone();
        finish_payload.insert(0, self.packet_id);

        buff.extend(SYNC_BYTES);
        buff.extend((finish_payload.len() as u16).to_le_bytes());
        buff.extend(&finish_payload);

        buff.push(crc8(&finish_payload));

        debug!("Sending packet: {:?}", buff);

        buff
    }
}

#[allow(unused)]
impl UARTPacket {
    /// Extends a reader position to a provided length and returns old value
    fn extend_reader(&mut self, len: usize) -> usize {
        let old = self._reader_pos;
        self._reader_pos += len;
        old
    }

    /// Read next u8
    pub fn read_u8(&mut self) -> Option<u8> {
        let pos = self.extend_reader(1);
        self.payload.get(pos).copied()
    }

    /// Read next boolean
    pub fn read_bool(&mut self) -> Option<bool> {
        let pos = self.extend_reader(1);
        self.payload.get(pos).map(|v| *v == 1)
    }

    /// Read next u16
    pub fn read_u16(&mut self) -> Option<u16> {
        let pos = self.extend_reader(2);
        Some(u16::from_le_bytes(
            self.payload.get(pos..pos + 2)?.try_into().ok()?,
        ))
    }

    /// Read next f64
    pub fn read_f64(&mut self) -> Option<f64> {
        let pos = self.extend_reader(8);
        Some(f64::from_le_bytes(
            self.payload.get(pos..pos + 8)?.try_into().ok()?,
        ))
    }
}

impl UARTPacket {
    /// Write u16 to end of packet payload
    pub fn write_u16(&mut self, val: u16) {
        self.payload.extend(val.to_le_bytes());
    }

    /// Write u8 to end of packet payload
    pub fn write_u8(&mut self, val: u8) {
        self.payload.extend(val.to_le_bytes());
    }

    /// Write u32 to end of packet payload
    pub fn write_u32(&mut self, val: u32) {
        self.payload.extend(val.to_le_bytes());
    }

    /// Write f32 to end of packet payload
    pub fn write_f32(&mut self, val: f32) {
        self.payload.extend(val.to_le_bytes());
    }

    /// Write f64 to end of packet payload
    pub fn write_f64(&mut self, val: f64) {
        self.payload.extend(val.to_le_bytes());
    }

    /// Write string
    ///
    /// Buffer layout: <len u16> <bytes>
    pub fn write_string(&mut self, str: impl Into<String>) {
        let s = str.into();
        self.write_u16(s.len() as u16);
        self.payload.extend(s.bytes());
    }

    /// Extends packet from other's packet data
    pub fn extend(&mut self, rh: Self) {
        self.payload.extend(rh.payload);
    }
}

#[cfg(test)]
mod tests {
    use super::UARTPacket;

    #[test]
    fn parse_accepts_packet_with_valid_crc() {
        let packet = UARTPacket::new(0x2A, &[1, 2, 3]);

        let parsed = UARTPacket::parse(packet.send()).expect("valid packet must parse");

        assert_eq!(parsed.packet_id, 0x2A);
        assert_eq!(parsed.payload, vec![1, 2, 3]);
    }

    #[test]
    fn parse_rejects_packet_with_invalid_crc() {
        let packet = UARTPacket::new(0x2A, &[1, 2, 3]);
        let mut encoded = packet.send();
        let crc_index = encoded.len() - 1;
        encoded[crc_index] ^= 0x01;

        let error = UARTPacket::parse(encoded).expect_err("corrupted packet must be rejected");

        assert!(error.to_string().contains("Invalid packet CRC"));
    }
}
