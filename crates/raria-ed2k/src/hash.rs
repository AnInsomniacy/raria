//! ED2K root hash, part hashset, and AICH ownership.

use md4::{Digest as Md4Digest, Md4};
use sha1::{Digest as Sha1Digest, Sha1};

/// ED2K protocol part size in bytes.
pub const ED2K_PART_SIZE: u64 = 9_728_000;
/// AICH leaf block size in bytes.
pub const AICH_EMBLOCK_SIZE: u64 = 184_320;
/// ED2K MD4 hash size in bytes.
pub const ED2K_HASH_SIZE: usize = 16;
/// AICH SHA-1 hash size in bytes.
pub const AICH_HASH_SIZE: usize = 20;

/// ED2K MD4 hash bytes.
pub type Ed2kHash = [u8; ED2K_HASH_SIZE];
/// AICH SHA-1 hash bytes.
pub type AichHash = [u8; AICH_HASH_SIZE];

/// ED2K hash and AICH validation error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Ed2kHashError {
    /// The provided part hash count does not match ED2K boundary rules.
    #[error("invalid ED2K part hash count: expected {expected}, got {actual}")]
    InvalidPartHashCount {
        /// Required number of part hashes.
        expected: u64,
        /// Provided number of part hashes.
        actual: u64,
    },
    /// The AICH Base32 text is malformed.
    #[error("invalid AICH Base32 root")]
    InvalidAichBase32,
    /// AICH recovery metadata is malformed.
    #[error("invalid AICH recovery data")]
    InvalidAichRecoveryData,
    /// AICH recovery metadata does not verify against the expected root.
    #[error("AICH recovery data does not match the expected root")]
    AichRecoveryMismatch,
}

/// Verified AICH recovery hashes for one ED2K part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AichRecoverySet {
    /// ED2K part index covered by this recovery set.
    pub part_index: u64,
    /// Verified AICH leaf blocks in part-relative order.
    pub blocks: Vec<AichRecoveryBlock>,
}

/// Verified AICH recovery leaf block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AichRecoveryBlock {
    /// Part-relative block offset in bytes.
    pub offset: u64,
    /// Block length in bytes.
    pub length: u64,
    /// AICH block hash.
    pub hash: AichHash,
}

/// Parsed AICH recovery metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AichRecoveryData {
    hashes16: Vec<AichRecoveryHash>,
    hashes32: Vec<AichRecoveryHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AichRecoveryHash {
    identifier: u32,
    hash: AichHash,
}

/// Return the MD4 digest used by ED2K.
pub fn md4_digest(data: &[u8]) -> Ed2kHash {
    let digest = Md4::digest(data);
    digest.into()
}

/// Return the number of part hashes carried by ED2K hashset messages.
pub fn theoretical_part_hash_count(size: u64) -> u64 {
    if size == 0 {
        return 0;
    }
    let full_parts = size / ED2K_PART_SIZE;
    if full_parts == 0 { 0 } else { full_parts + 1 }
}

/// Compute an ED2K root hash from a complete byte slice.
pub fn ed2k_root_hash(data: &[u8]) -> Ed2kHash {
    if theoretical_part_hash_count(data.len() as u64) == 0 {
        return md4_digest(data);
    }

    let mut part_hashes = Vec::new();
    for chunk in data.chunks(ED2K_PART_SIZE as usize) {
        part_hashes.push(md4_digest(chunk));
    }
    if data.len() % ED2K_PART_SIZE as usize == 0 {
        part_hashes.push(md4_digest(b""));
    }
    root_hash_from_hashes(&part_hashes)
}

/// Compute an ED2K root hash from validated part hashes.
pub fn ed2k_root_hash_from_part_hashes(
    size: u64,
    part_hashes: &[Ed2kHash],
) -> Result<Ed2kHash, Ed2kHashError> {
    let expected = theoretical_part_hash_count(size);
    let actual = part_hashes.len() as u64;
    if actual != expected {
        return Err(Ed2kHashError::InvalidPartHashCount { expected, actual });
    }
    Ok(root_hash_from_hashes(part_hashes))
}

/// Return the SHA-1 digest used by AICH.
pub fn aich_hash(data: &[u8]) -> AichHash {
    let digest = Sha1::digest(data);
    digest.into()
}

/// Compute the AICH root for complete file bytes.
pub fn aich_root_hash(data: &[u8]) -> AichHash {
    let base_size = if data.len() as u64 <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    aich_root_hash_for_range(data, base_size, true)
}

/// Parse a canonical AICH Base32 root.
pub fn parse_aich_root_base32(text: &str) -> Result<AichHash, Ed2kHashError> {
    let text = text.trim().as_bytes();
    if text.len() != 32 {
        return Err(Ed2kHashError::InvalidAichBase32);
    }

    let mut output = [0_u8; AICH_HASH_SIZE];
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    let mut written = 0_usize;
    for &byte in text {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err(Ed2kHashError::InvalidAichBase32),
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            if written >= output.len() {
                return Err(Ed2kHashError::InvalidAichBase32);
            }
            output[written] = (buffer >> bits) as u8;
            written += 1;
            buffer &= (1 << bits) - 1;
        }
    }
    if written != output.len() || bits != 0 {
        return Err(Ed2kHashError::InvalidAichBase32);
    }
    Ok(output)
}

/// Format an AICH root as uppercase unpadded Base32.
pub fn aich_root_base32(hash: AichHash) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::with_capacity(32);
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in hash {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
            buffer &= (1 << bits) - 1;
        }
    }
    debug_assert_eq!(bits, 0);
    output
}

/// Parse AICH recovery metadata for one part.
pub fn parse_aich_recovery_data(
    payload: &[u8],
    part_size: u64,
    large_file: bool,
) -> Result<AichRecoveryData, Ed2kHashError> {
    if part_size <= AICH_EMBLOCK_SIZE {
        return Err(Ed2kHashError::InvalidAichRecoveryData);
    }

    let mut cursor = Cursor::new(payload);
    let count16 = cursor.read_u16()?;
    let mut hashes16 = Vec::with_capacity(count16 as usize);
    for _ in 0..count16 {
        let identifier = u32::from(cursor.read_u16()?);
        if !valid_aich_identifier(identifier) {
            return Err(Ed2kHashError::InvalidAichRecoveryData);
        }
        hashes16.push(AichRecoveryHash {
            identifier,
            hash: cursor.read_aich_hash()?,
        });
    }

    let mut hashes32 = Vec::new();
    if cursor.remaining() > 0 {
        let count32 = cursor.read_u16()?;
        if count32 != 0 && !large_file {
            return Err(Ed2kHashError::InvalidAichRecoveryData);
        }
        hashes32.reserve(count32 as usize);
        for _ in 0..count32 {
            let identifier = cursor.read_u32()?;
            if !valid_aich_identifier(identifier) {
                return Err(Ed2kHashError::InvalidAichRecoveryData);
            }
            hashes32.push(AichRecoveryHash {
                identifier,
                hash: cursor.read_aich_hash()?,
            });
        }
    }

    if cursor.remaining() != 0 || hashes16.is_empty() && hashes32.is_empty() {
        return Err(Ed2kHashError::InvalidAichRecoveryData);
    }
    Ok(AichRecoveryData { hashes16, hashes32 })
}

/// Verify AICH recovery metadata and return block hashes for the target part.
pub fn build_aich_recovery_set(
    recovery: &AichRecoveryData,
    root_hash: AichHash,
    file_size: u64,
    part_index: u64,
) -> Result<AichRecoverySet, Ed2kHashError> {
    let part_size = aich_part_size(file_size, part_index);
    if part_size <= AICH_EMBLOCK_SIZE {
        return Err(Ed2kHashError::InvalidAichRecoveryData);
    }

    let file_base = if file_size <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    let part_offset = part_index
        .checked_mul(ED2K_PART_SIZE)
        .ok_or(Ed2kHashError::InvalidAichRecoveryData)?;
    let part_hash =
        recovery_root_for_range(recovery, part_offset, part_size, file_size, file_base, true)
            .ok_or(Ed2kHashError::AichRecoveryMismatch)?;
    let full_hash = recovery_root_for_range(recovery, 0, file_size, file_size, file_base, true)
        .ok_or(Ed2kHashError::AichRecoveryMismatch)?;
    if full_hash != root_hash || part_hash.is_empty() {
        return Err(Ed2kHashError::AichRecoveryMismatch);
    }

    let mut blocks = Vec::new();
    let mut offset = 0_u64;
    while offset < part_size {
        let length = AICH_EMBLOCK_SIZE.min(part_size - offset);
        let hash = hash_for_range(
            recovery,
            part_offset + offset,
            length,
            file_size,
            file_base,
            true,
        )
        .ok_or(Ed2kHashError::AichRecoveryMismatch)?;
        blocks.push(AichRecoveryBlock {
            offset,
            length,
            hash,
        });
        offset += length;
    }

    Ok(AichRecoverySet { part_index, blocks })
}

fn root_hash_from_hashes(part_hashes: &[Ed2kHash]) -> Ed2kHash {
    match part_hashes {
        [] => md4_digest(b""),
        [single] => *single,
        many => {
            let mut concat = Vec::with_capacity(many.len() * ED2K_HASH_SIZE);
            for hash in many {
                concat.extend_from_slice(hash);
            }
            md4_digest(&concat)
        }
    }
}

fn aich_root_hash_for_range(data: &[u8], base_size: u64, left_branch: bool) -> AichHash {
    if data.len() as u64 <= base_size {
        return aich_hash(data);
    }
    let blocks = data.len().div_ceil(base_size as usize);
    let left_blocks = if left_branch {
        blocks.div_ceil(2)
    } else {
        blocks / 2
    };
    let left_len = data.len().min(left_blocks * base_size as usize);
    let right_len = data.len() - left_len;
    let left_base = if left_len as u64 <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    let right_base = if right_len as u64 <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    let left_hash = aich_root_hash_for_range(&data[..left_len], left_base, true);
    let right_hash = aich_root_hash_for_range(&data[left_len..], right_base, false);
    aich_hash(&[left_hash.as_slice(), right_hash.as_slice()].concat())
}

fn aich_part_size(file_size: u64, part_index: u64) -> u64 {
    let begin = part_index.saturating_mul(ED2K_PART_SIZE);
    if begin >= file_size {
        0
    } else {
        ED2K_PART_SIZE.min(file_size - begin)
    }
}

fn valid_aich_identifier(identifier: u32) -> bool {
    identifier != 1 && (2..=0x400000).contains(&identifier)
}

fn hash_for_range(
    recovery: &AichRecoveryData,
    target_offset: u64,
    target_size: u64,
    data_size: u64,
    base_size: u64,
    left_branch: bool,
) -> Option<AichHash> {
    for item in recovery.hashes16.iter().chain(recovery.hashes32.iter()) {
        if identifier_path_reaches_range(
            item.identifier,
            target_offset,
            target_size,
            data_size,
            base_size,
            left_branch,
        ) {
            return Some(item.hash);
        }
    }
    None
}

fn recovery_root_for_range(
    recovery: &AichRecoveryData,
    target_offset: u64,
    target_size: u64,
    data_size: u64,
    base_size: u64,
    left_branch: bool,
) -> Option<AichHash> {
    if let Some(hash) = hash_for_range(
        recovery,
        target_offset,
        target_size,
        data_size,
        base_size,
        true,
    ) {
        return Some(hash);
    }
    if target_size <= base_size {
        return None;
    }

    let blocks = target_size.div_ceil(base_size);
    let left_blocks = if left_branch {
        blocks.div_ceil(2)
    } else {
        blocks / 2
    };
    let left_size = target_size.min(left_blocks * base_size);
    let right_size = target_size - left_size;
    let left_base = if left_size <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    let right_base = if right_size <= ED2K_PART_SIZE {
        AICH_EMBLOCK_SIZE
    } else {
        ED2K_PART_SIZE
    };
    let left = recovery_root_for_range(
        recovery,
        target_offset,
        left_size,
        data_size,
        left_base,
        true,
    )?;
    let right = recovery_root_for_range(
        recovery,
        target_offset + left_size,
        right_size,
        data_size,
        right_base,
        false,
    )?;
    Some(aich_hash(&[left.as_slice(), right.as_slice()].concat()))
}

fn identifier_path_reaches_range(
    identifier: u32,
    target_offset: u64,
    target_size: u64,
    data_size: u64,
    base_size: u64,
    left_branch: bool,
) -> bool {
    if identifier == 0
        || data_size == 0
        || target_size == 0
        || target_offset.saturating_add(target_size) > data_size
    {
        return false;
    }

    let mut bit = 0x8000_0000_u32;
    while bit != 0 && identifier & bit == 0 {
        bit >>= 1;
    }
    if bit == 0 {
        return false;
    }
    if bit == 1 {
        return target_offset == 0 && target_size == data_size;
    }
    bit >>= 1;

    let mut offset = 0_u64;
    let mut length = data_size;
    let mut current_base = base_size;
    let mut current_left = left_branch;
    while bit != 0 && length > current_base {
        let blocks = length.div_ceil(current_base);
        let left_blocks = if current_left {
            blocks.div_ceil(2)
        } else {
            blocks / 2
        };
        let left_length = length.min(left_blocks * current_base);
        let right_length = length - left_length;
        if identifier & bit != 0 {
            length = left_length;
            current_left = true;
        } else {
            offset += left_length;
            length = right_length;
            current_left = false;
        }
        current_base = if length <= ED2K_PART_SIZE {
            AICH_EMBLOCK_SIZE
        } else {
            ED2K_PART_SIZE
        };
        bit >>= 1;
    }
    offset == target_offset && length == target_size && bit == 0
}

struct Cursor<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.payload.len().saturating_sub(self.offset)
    }

    fn read_u16(&mut self) -> Result<u16, Ed2kHashError> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, Ed2kHashError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_aich_hash(&mut self) -> Result<AichHash, Ed2kHashError> {
        let bytes = self.read_exact(AICH_HASH_SIZE)?;
        let mut hash = [0_u8; AICH_HASH_SIZE];
        hash.copy_from_slice(bytes);
        Ok(hash)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], Ed2kHashError> {
        if self.remaining() < len {
            return Err(Ed2kHashError::InvalidAichRecoveryData);
        }
        let start = self.offset;
        self.offset += len;
        Ok(&self.payload[start..self.offset])
    }
}

#[cfg(test)]
fn hex_hash(value: &str) -> Ed2kHash {
    let mut output = [0_u8; ED2K_HASH_SIZE];
    for index in 0..ED2K_HASH_SIZE {
        output[index] = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("hex hash");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeated(byte: u8, len: usize) -> Vec<u8> {
        vec![byte; len]
    }

    #[test]
    fn computes_md4_digest_vectors() {
        assert_eq!(
            md4_digest(b""),
            hex_hash("31d6cfe0d16ae931b73c59d7e0c089c0")
        );
        assert_eq!(
            md4_digest(b"abc"),
            hex_hash("a448017aaf21d8525fc10ae87aa6729d")
        );
        assert_eq!(
            md4_digest(b"message digest"),
            hex_hash("d9130a8164549fe818874806e1c7014b")
        );
    }

    #[test]
    fn counts_ed2k_part_hashes_at_protocol_boundaries() {
        assert_eq!(theoretical_part_hash_count(0), 0);
        assert_eq!(theoretical_part_hash_count(1), 0);
        assert_eq!(theoretical_part_hash_count(ED2K_PART_SIZE - 1), 0);
        assert_eq!(theoretical_part_hash_count(ED2K_PART_SIZE), 2);
        assert_eq!(theoretical_part_hash_count(ED2K_PART_SIZE + 1), 2);
        assert_eq!(theoretical_part_hash_count(ED2K_PART_SIZE * 2), 3);
        assert_eq!(theoretical_part_hash_count(ED2K_PART_SIZE * 2 + 1), 3);
    }

    #[test]
    fn computes_ed2k_root_hash_for_small_and_boundary_files() {
        assert_eq!(ed2k_root_hash(b"abc"), md4_digest(b"abc"));

        let exact_part = repeated(b'a', ED2K_PART_SIZE as usize);
        let exact_part_hash = md4_digest(&exact_part);
        let empty_hash = md4_digest(b"");
        assert_eq!(
            ed2k_root_hash(&exact_part),
            md4_digest(&[exact_part_hash.as_slice(), empty_hash.as_slice()].concat())
        );

        let mut part_plus_one = exact_part;
        part_plus_one.push(b'b');
        let first = md4_digest(&part_plus_one[..ED2K_PART_SIZE as usize]);
        let second = md4_digest(&part_plus_one[ED2K_PART_SIZE as usize..]);
        assert_eq!(
            ed2k_root_hash(&part_plus_one),
            md4_digest(&[first.as_slice(), second.as_slice()].concat())
        );
    }

    #[test]
    fn validates_root_hash_from_part_hashes() {
        let first = md4_digest(b"first part");
        let second = md4_digest(b"second part");
        let root = ed2k_root_hash_from_part_hashes(ED2K_PART_SIZE + 1, &[first, second])
            .expect("root hash");

        assert_eq!(
            root,
            md4_digest(&[first.as_slice(), second.as_slice()].concat())
        );
        assert_eq!(
            ed2k_root_hash_from_part_hashes(ED2K_PART_SIZE + 1, &[first])
                .expect_err("part hash count"),
            Ed2kHashError::InvalidPartHashCount {
                expected: 2,
                actual: 1,
            }
        );
    }

    #[test]
    fn parses_and_formats_aich_base32_roots() {
        let parsed = parse_aich_root_base32("abcdefghijklmnopqrstuvwxyz234567").expect("aich root");
        assert_eq!(aich_root_base32(parsed), "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567");
        assert!(parse_aich_root_base32("ABC").is_err());
        assert!(parse_aich_root_base32("ABCDEFGHIJKLMNOPQRSTUVWXYZ234568").is_err());
    }

    #[test]
    fn computes_aich_root_tree_for_block_and_part_boundaries() {
        let block0 = repeated(b'a', AICH_EMBLOCK_SIZE as usize);
        let block1 = repeated(b'b', AICH_EMBLOCK_SIZE as usize);
        let block2 = repeated(b'c', 100);
        let mut data = Vec::new();
        data.extend_from_slice(&block0);
        data.extend_from_slice(&block1);
        data.extend_from_slice(&block2);

        let left =
            aich_hash(&[aich_hash(&block0).as_slice(), aich_hash(&block1).as_slice()].concat());
        let expected = aich_hash(&[left.as_slice(), aich_hash(&block2).as_slice()].concat());
        assert_eq!(aich_root_hash(&data), expected);

        let first_part = repeated(b'a', ED2K_PART_SIZE as usize);
        let second_part = repeated(b'b', AICH_EMBLOCK_SIZE as usize);
        let mut multi_part = Vec::new();
        multi_part.extend_from_slice(&first_part);
        multi_part.extend_from_slice(&second_part);
        let expected = aich_hash(
            &[
                aich_root_hash(&first_part).as_slice(),
                aich_root_hash(&second_part).as_slice(),
            ]
            .concat(),
        );
        assert_eq!(aich_root_hash(&multi_part), expected);
    }

    #[test]
    fn parses_and_verifies_aich_recovery_metadata() {
        let block0 = repeated(b'a', AICH_EMBLOCK_SIZE as usize);
        let block1 = repeated(b'b', AICH_EMBLOCK_SIZE as usize);
        let block2 = repeated(b'c', 100);
        let mut data = Vec::new();
        data.extend_from_slice(&block0);
        data.extend_from_slice(&block1);
        data.extend_from_slice(&block2);
        let root = aich_root_hash(&data);

        let mut payload = Vec::new();
        payload.extend_from_slice(&3_u16.to_le_bytes());
        payload.extend_from_slice(&7_u16.to_le_bytes());
        payload.extend_from_slice(&aich_hash(&block0));
        payload.extend_from_slice(&6_u16.to_le_bytes());
        payload.extend_from_slice(&aich_hash(&block1));
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&aich_hash(&block2));
        payload.extend_from_slice(&0_u16.to_le_bytes());

        let recovery =
            parse_aich_recovery_data(&payload, data.len() as u64, false).expect("recovery data");
        let set =
            build_aich_recovery_set(&recovery, root, data.len() as u64, 0).expect("verified set");
        assert_eq!(set.part_index, 0);
        assert_eq!(set.blocks.len(), 3);
        assert_eq!(set.blocks[1].hash, aich_hash(&block1));

        let mut corrupted = payload;
        corrupted[4] ^= 0x01;
        let recovery = parse_aich_recovery_data(&corrupted, data.len() as u64, false)
            .expect("corrupted recovery parses");
        assert!(build_aich_recovery_set(&recovery, root, data.len() as u64, 0).is_err());
    }
}
