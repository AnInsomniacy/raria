use crate::hash::{ED2K_HASH_SIZE, Ed2kHash};

pub(crate) struct Cursor<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.payload.len().saturating_sub(self.offset)
    }

    pub(crate) fn is_done(&self) -> bool {
        self.offset == self.payload.len()
    }

    pub(crate) fn position(&self) -> usize {
        self.offset
    }

    pub(crate) fn read_u8(&mut self) -> Option<u8> {
        self.read_exact(1).map(|bytes| bytes[0])
    }

    pub(crate) fn read_u16(&mut self) -> Option<u16> {
        let bytes = self.read_exact(2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn read_u32(&mut self) -> Option<u32> {
        let bytes = self.read_exact(4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn read_u64(&mut self) -> Option<u64> {
        let bytes = self.read_exact(8)?;
        Some(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(crate) fn read_hash16(&mut self) -> Option<Ed2kHash> {
        let bytes = self.read_exact(ED2K_HASH_SIZE)?;
        let mut hash = [0_u8; ED2K_HASH_SIZE];
        hash.copy_from_slice(bytes);
        Some(hash)
    }

    pub(crate) fn read_exact(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.remaining() < len {
            return None;
        }
        let start = self.offset;
        self.offset += len;
        Some(&self.payload[start..self.offset])
    }
}

pub(crate) fn ipv4_from_server_met(value: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        value & 0xff,
        (value >> 8) & 0xff,
        (value >> 16) & 0xff,
        (value >> 24) & 0xff
    )
}

pub(crate) fn ipv4_from_kad_contact(value: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (value >> 24) & 0xff,
        (value >> 16) & 0xff,
        (value >> 8) & 0xff,
        value & 0xff
    )
}
