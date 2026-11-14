//! Deterministic test bytes and whole sets.

use reed_solomon_core::{Code, Field, Zeroizing};

/// Every field.
pub(crate) const FIELDS: [Field; 3] = [Field::Gf256, Field::Gf65536, Field::Gf2048];

/// Bytes from a fixed generator (SplitMix64).
pub(crate) fn bytes(seed: u64, count: usize) -> Vec<u8> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as u8
        })
        .collect()
}

/// `bytes` as symbols of `field`: a word is below 2,048, so the five high bits of each go.
pub(crate) fn symbols(field: Field, mut bytes: Vec<u8>) -> Vec<u8> {
    if field == Field::Gf2048 {
        for pair in bytes.as_chunks_mut::<2>().0 {
            pair[0] &= 0x07;
        }
    }
    bytes
}

/// `k` data shards of `length` bytes of symbols of `field`.
pub(crate) fn data_in(field: Field, k: usize, length: usize, seed: u64) -> Vec<Vec<u8>> {
    data(k, length, seed)
        .into_iter()
        .map(|shard| symbols(field, shard))
        .collect()
}

/// `k` data shards of `length` bytes.
pub(crate) fn data(k: usize, length: usize, seed: u64) -> Vec<Vec<u8>> {
    (0..k)
        .map(|i| bytes(seed.wrapping_mul(1000) + i as u64, length))
        .collect()
}

/// The first `n` shards of the set with these data shards.
pub(crate) fn set(code: &Code, data: &[Vec<u8>], n: usize) -> Vec<Vec<u8>> {
    let data: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
    let targets: Vec<u16> = (0..n).map(|i| u16::try_from(i).unwrap()).collect();
    plain(code.encode(&data, &targets).unwrap())
}

/// Owned results as plain vectors, to compare with bytes written out in a test.
pub(crate) fn plain(shards: Vec<Zeroizing<Vec<u8>>>) -> Vec<Vec<u8>> {
    shards.into_iter().map(|shard| shard.to_vec()).collect()
}

/// The shards numbered `picked`, as `(index, bytes)`.
pub(crate) fn pick<'a>(set: &'a [Vec<u8>], picked: &[usize]) -> Vec<(u16, &'a [u8])> {
    picked
        .iter()
        .map(|&i| (u16::try_from(i).unwrap(), set[i].as_slice()))
        .collect()
}

/// CRC-32C by a long division with the polynomial as it is written, 0x1EDC6F41, the high bit
/// first, over bytes whose bits are reversed, with the result reversed: another route to the
/// checksum than the crate's bit-by-bit one over the reflected polynomial. A test checks it
/// against the published value for "123456789" before anything relies on it.
pub(crate) fn crc32c(bytes: &[u8]) -> u32 {
    let mut register = u32::MAX;
    for &byte in bytes {
        register ^= u32::from(byte.reverse_bits()) << 24;
        for _ in 0..8 {
            let high = register & 0x8000_0000 != 0;
            register <<= 1;
            if high {
                register ^= 0x1EDC_6F41;
            }
        }
    }
    (!register).reverse_bits()
}

/// The fields of a shard file's header as raw numbers, for files written apart from the crate:
/// also ones the crate refuses to write.
#[derive(Clone)]
pub(crate) struct RawHeader {
    pub(crate) version: u8,
    pub(crate) field: u8,
    pub(crate) flags: u8,
    pub(crate) id: Vec<u8>,
    pub(crate) threshold: u32,
    pub(crate) index: u32,
    pub(crate) data_len: u64,
    pub(crate) block_len: u32,
    pub(crate) digest: Option<[u8; 32]>,
}

impl RawHeader {
    /// The header's bytes, with its checksum.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut header = b"SHRD".to_vec();
        header.extend([self.version, self.field, self.flags]);
        header.push(u8::try_from(self.id.len()).unwrap());
        header.extend(&self.id);
        header.extend(self.threshold.to_be_bytes());
        header.extend(self.index.to_be_bytes());
        header.extend(self.data_len.to_be_bytes());
        header.extend(self.block_len.to_be_bytes());
        header.extend(self.digest.iter().flatten());
        header.extend(crc32c(&header).to_be_bytes());
        header
    }

    /// The whole file of `shard`: the header, the blocks with a checksum each, the header again
    /// and the trailer.
    pub(crate) fn file(&self, shard: &[u8]) -> Vec<u8> {
        let header = self.bytes();
        let own = header[header.len() - 4..].to_vec();
        let mut file = header.clone();
        let block_len = usize::try_from(self.block_len).unwrap();
        for (number, block) in shard.chunks(block_len.max(1)).enumerate() {
            let number = u32::try_from(number).unwrap().to_be_bytes();
            file.extend(block);
            file.extend(crc32c(&[&own[..], &number[..], block].concat()).to_be_bytes());
        }
        file.extend(&header);
        file.extend(trailer(header.len()));
        file
    }
}

/// The trailer of a file whose header is `header_len` bytes long.
pub(crate) fn trailer(header_len: usize) -> Vec<u8> {
    let mut trailer = b"SHRT".to_vec();
    trailer.extend(u32::try_from(header_len).unwrap().to_be_bytes());
    trailer.extend(crc32c(&trailer).to_be_bytes());
    trailer
}

/// Bytes from their hexadecimal digits; spaces are skipped.
pub(crate) fn hex(digits: &str) -> Vec<u8> {
    let digits: Vec<u32> = digits
        .chars()
        .filter_map(|digit| digit.to_digit(16))
        .collect();
    digits
        .chunks(2)
        .map(|pair| u8::try_from(pair[0] * 16 + pair[1]).unwrap())
        .collect()
}
