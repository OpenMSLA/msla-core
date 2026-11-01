pub struct DataWriter {
    pub payload: Vec<u8>,
}

impl DataWriter {
    pub fn new() -> Self {
        Self {
            payload: Vec::new(),
        }
    }

    /// Writes u8 to the end
    pub fn write_u8(&mut self, x: u8) {
        self.payload.push(x);
    }

    /// Writes string to the end
    pub fn write_string(&mut self, s: impl Into<String>) {
        let str = s.into();

        self.write_u8(str.len() as u8);
        self.payload.extend(str.bytes());
    }

    /// Get writer data as bytes
    pub fn send<'a>(&'a self) -> &'a [u8] {
        &self.payload
    }
}
