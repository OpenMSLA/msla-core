pub struct ByteReader {
    pub payload: Vec<u8>,
    pub read_pos: usize,
}

impl ByteReader {
    /// Init byte reader from slice
    pub fn from(bytes: &[u8], size: usize) -> Self {
        Self {
            payload: Vec::from(&bytes[..size]),
            read_pos: 0,
        }
    }

    pub fn validate_start_move(&mut self) -> bool {
        self.read_pos += 3;
        self.payload.starts_with(&[0xAA, 0x55, 0x02])
    }

    /// Reads next u8
    pub fn read_u8(&mut self) -> Option<u8> {
        let p = self.read_pos;
        self.read_pos += 1;
        self.payload.get(p).copied()
    }

    /// Reads next string
    pub fn read_string(&mut self) -> Option<String> {
        let len = self.read_u8()? as usize;

        if self.payload.len() < self.read_pos + len {
            return None;
        }

        let pos: usize = self.read_pos;
        self.read_pos += len;

        Some(String::from_utf8_lossy(&self.payload[pos..pos + len]).into_owned())
    }
}
