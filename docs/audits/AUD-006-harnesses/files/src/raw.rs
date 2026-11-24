//! Raw shard-file bytes written apart from the crate, for probes.

pub fn crc32c(parts: &[&[u8]]) -> u32 {
    let mut crc = u32::MAX;
    for part in parts {
        for &byte in *part {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0x82F6_3B78 } else { crc >> 1 };
            }
        }
    }
    !crc
}

#[derive(Clone, Debug)]
pub struct Raw {
    pub version: u8,
    pub field: u8,
    pub flags: u8,
    pub id: Vec<u8>,
    pub threshold: u32,
    pub index: u32,
    pub data_len: u64,
    pub block_len: u32,
    pub digest: Option<[u8; 32]>,
}

impl Raw {
    pub fn new(threshold: u32, index: u32, data_len: u64, block_len: u32) -> Self {
        Self {
            version: 1,
            field: 1,
            flags: 0,
            id: vec![1],
            threshold,
            index,
            data_len,
            block_len,
            digest: None,
        }
    }

    pub fn header(&self) -> Vec<u8> {
        let mut h = b"SHRD".to_vec();
        h.extend([self.version, self.field, self.flags, self.id.len() as u8]);
        h.extend(&self.id);
        h.extend(self.threshold.to_be_bytes());
        h.extend(self.index.to_be_bytes());
        h.extend(self.data_len.to_be_bytes());
        h.extend(self.block_len.to_be_bytes());
        if let Some(d) = self.digest {
            h.extend(d);
        }
        let sum = crc32c(&[&h]);
        h.extend(sum.to_be_bytes());
        h
    }

    pub fn own(&self) -> [u8; 4] {
        let h = self.header();
        h[h.len() - 4..].try_into().unwrap()
    }

    pub fn block(&self, number: u32, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out.extend(crc32c(&[&self.own(), &number.to_be_bytes(), bytes]).to_be_bytes());
        out
    }

    /// The whole file of `shard`, cut into blocks of `block_len`.
    pub fn file(&self, shard: &[u8]) -> Vec<u8> {
        let h = self.header();
        let mut f = h.clone();
        for (n, b) in shard.chunks(self.block_len.max(1) as usize).enumerate() {
            f.extend(self.block(n as u32, b));
        }
        f.extend(&h);
        f.extend(trailer(h.len() as u32));
        f
    }
}

pub fn trailer(len: u32) -> Vec<u8> {
    let mut t = b"SHRT".to_vec();
    t.extend(len.to_be_bytes());
    let sum = crc32c(&[&t]);
    t.extend(sum.to_be_bytes());
    t
}
