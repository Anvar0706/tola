//! Flat append-only file segment storage for immutable blocks, transactions, and receipts.
//!
//! Complies with ADR-0008:
//! - Eliminates LSM/B+Tree write amplification for immutable historical blocks
//! - BLAKE3 checksum verification on every read to detect disk rot or tampering
//! - Automatic segment rollover upon reaching configured byte threshold

use crate::kv::StorageError;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tola_crypto::blake3_hash;

pub const MAGIC_TOLA_SEGMENT: [u8; 4] = [0x54, 0x4F, 0x4C, 0x41]; // "TOLA"
pub const DEFAULT_MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024; // 64 MB per segment file

pub const RECORD_BLOCK_HEADER: u8 = 0x01;
pub const RECORD_BLOCK_BODY: u8 = 0x02;
pub const RECORD_RECEIPT: u8 = 0x03;

/// Pointer to exact location of an immutable record within segment files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentLocation {
    pub segment_id: u32,
    pub byte_offset: u64,
    pub payload_len: u32,
}

impl SegmentLocation {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.segment_id.to_le_bytes());
        b[4..12].copy_from_slice(&self.byte_offset.to_le_bytes());
        b[12..16].copy_from_slice(&self.payload_len.to_le_bytes());
        b
    }

    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        let segment_id = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let byte_offset = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
        let payload_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        Self {
            segment_id,
            byte_offset,
            payload_len,
        }
    }
}

pub struct SegmentStorage {
    dir: PathBuf,
    current_segment_id: u32,
    current_file: File,
    current_offset: u64,
    max_segment_bytes: u64,
}

impl SegmentStorage {
    /// Open segment storage inside target directory.
    pub fn open(path: impl AsRef<Path>, max_segment_bytes: u64) -> Result<Self, StorageError> {
        let dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        // Discover latest existing segment
        let mut latest_id = 0;
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("segment_") && name.ends_with(".seg") {
                    if let Ok(id) = name[8..13].parse::<u32>() {
                        if id > latest_id {
                            latest_id = id;
                        }
                    }
                }
            }
        }

        let seg_path = dir.join(format!("segment_{latest_id:05}.seg"));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&seg_path)?;

        let current_offset = file.seek(SeekFrom::End(0))?;

        Ok(Self {
            dir,
            current_segment_id: latest_id,
            current_file: file,
            current_offset,
            max_segment_bytes,
        })
    }

    /// Append record to current segment, rolling over to new segment if threshold exceeded.
    pub fn append(
        &mut self,
        record_type: u8,
        payload: &[u8],
    ) -> Result<SegmentLocation, StorageError> {
        let record_size = 4 + 4 + 1 + 32 + payload.len() as u64;

        if self.current_offset > 0 && (self.current_offset + record_size) > self.max_segment_bytes {
            // Rollover to next segment
            self.current_segment_id += 1;
            let seg_path = self
                .dir
                .join(format!("segment_{:05}.seg", self.current_segment_id));
            self.current_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&seg_path)?;
            self.current_offset = 0;
        }

        let start_offset = self.current_offset;
        let payload_len = payload.len() as u32;
        let checksum = blake3_hash(payload);

        let mut header = Vec::with_capacity(41);
        header.extend_from_slice(&MAGIC_TOLA_SEGMENT);
        header.extend_from_slice(&payload_len.to_le_bytes());
        header.push(record_type);
        header.extend_from_slice(checksum.as_bytes());

        self.current_file.write_all(&header)?;
        self.current_file.write_all(payload)?;

        self.current_offset += header.len() as u64 + payload.len() as u64;

        Ok(SegmentLocation {
            segment_id: self.current_segment_id,
            byte_offset: start_offset,
            payload_len,
        })
    }

    /// Read and cryptographically verify record at location.
    pub fn read(&self, loc: &SegmentLocation) -> Result<Vec<u8>, StorageError> {
        let seg_path = self.dir.join(format!("segment_{:05}.seg", loc.segment_id));
        let mut file = OpenOptions::new().read(true).open(&seg_path)?;

        file.seek(SeekFrom::Start(loc.byte_offset))?;

        let mut header = [0u8; 41];
        file.read_exact(&mut header)?;

        if header[0..4] != MAGIC_TOLA_SEGMENT {
            return Err(StorageError::CorruptedRecord);
        }

        let stored_len = u32::from_le_bytes(header[4..8].try_into().unwrap());
        if stored_len != loc.payload_len {
            return Err(StorageError::CorruptedRecord);
        }

        let stored_checksum = &header[9..41];

        let mut payload = vec![0u8; loc.payload_len as usize];
        file.read_exact(&mut payload)?;

        // Verify cryptographic integrity
        let computed_checksum = blake3_hash(&payload);
        if computed_checksum.as_bytes() != stored_checksum {
            return Err(StorageError::ChecksumMismatch);
        }

        Ok(payload)
    }

    /// Synchronize current segment to physical storage.
    pub fn sync(&mut self) -> Result<(), StorageError> {
        self.current_file.flush()?;
        self.current_file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_append_read_and_integrity() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_test_seg_{nanos}"));

        let mut storage = SegmentStorage::open(&temp_dir, 1024).expect("Open segments");

        let payload_1 = b"Block header height 1 with transactions".to_vec();
        let payload_2 = b"Block header height 2 with extra data".to_vec();

        let loc1 = storage.append(RECORD_BLOCK_HEADER, &payload_1).unwrap();
        let loc2 = storage.append(RECORD_BLOCK_HEADER, &payload_2).unwrap();

        assert_eq!(storage.read(&loc1).unwrap(), payload_1);
        assert_eq!(storage.read(&loc2).unwrap(), payload_2);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_segment_bit_rot_detection() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_test_seg_corrupt_{nanos}"));

        let mut storage = SegmentStorage::open(&temp_dir, 1024).unwrap();
        let payload = b"Sensitive ledger data".to_vec();
        let loc = storage.append(RECORD_BLOCK_BODY, &payload).unwrap();
        storage.sync().unwrap();

        // Deliberately corrupt 1 byte in the segment file payload
        let seg_path = temp_dir.join(format!("segment_{:05}.seg", loc.segment_id));
        let mut file = OpenOptions::new().write(true).open(&seg_path).unwrap();
        file.seek(SeekFrom::Start(loc.byte_offset + 41 + 2))
            .unwrap(); // Corrupt payload byte
        file.write_all(&[0xFF]).unwrap();
        file.sync_all().unwrap();

        // Attempting to read must return ChecksumMismatch!
        let err = storage.read(&loc).unwrap_err();
        assert_eq!(err, StorageError::ChecksumMismatch);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_segment_rollover() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_test_seg_rollover_{nanos}"));

        // Max segment size: 100 bytes
        let mut storage = SegmentStorage::open(&temp_dir, 100).unwrap();

        let payload = vec![0xAA; 50]; // Header is 41B + 50B = 91B
        let loc1 = storage.append(RECORD_BLOCK_HEADER, &payload).unwrap();
        assert_eq!(loc1.segment_id, 0);

        // Next write will exceed 100B, causing rollover to segment 1!
        let loc2 = storage.append(RECORD_BLOCK_HEADER, &payload).unwrap();
        assert_eq!(loc2.segment_id, 1);

        assert_eq!(storage.read(&loc1).unwrap(), payload);
        assert_eq!(storage.read(&loc2).unwrap(), payload);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
