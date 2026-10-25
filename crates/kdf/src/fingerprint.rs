//! KDF SQLite fingerprint removal (docs/format.md §1.1, `kpf_container.py`
//! `SQLiteFingerprintWrapper`).
//!
//! A fingerprinted file has a 1 KiB record with signature `FA 50 0A 5F` at
//! offset 1024, and then one more every 1 MiB of real data. Removing them
//! yields a plain SQLite database.

use std::io::{self, Write};

pub const SIGNATURE: [u8; 4] = [0xfa, 0x50, 0x0a, 0x5f];
const FIRST_OFFSET: usize = 1024;
pub const RECORD_LEN: usize = 1024;
const DATA_LEN: usize = 1024 * 1024;

/// Whether `data` (or at least its first 1028 bytes) carries fingerprints.
pub fn is_fingerprinted(data: &[u8]) -> bool {
    data.len() >= FIRST_OFFSET + RECORD_LEN && data[FIRST_OFFSET..FIRST_OFFSET + 4] == SIGNATURE
}

/// Offsets, in `data`, of the fingerprint records to remove; `None` if a record
/// is missing its signature (kfxlib then keeps the file unchanged).
pub fn fingerprint_offsets(data: &[u8]) -> Option<Vec<usize>> {
    if !is_fingerprinted(data) {
        return Some(Vec::new());
    }
    // kfxlib steps 1 MiB through the data *after* each removal, which in the
    // original file is a stride of 1 MiB + the removed record.
    let mut offsets = Vec::new();
    let mut pos = FIRST_OFFSET;
    while pos + RECORD_LEN <= data.len() {
        if data[pos..pos + 4] != SIGNATURE {
            return None;
        }
        offsets.push(pos);
        pos += DATA_LEN + RECORD_LEN;
    }
    Some(offsets)
}

/// Where the fingerprint records of a fingerprinted file of `len` bytes must be
/// (each still has to be checked for its signature).
pub fn offsets_for_len(len: usize) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut pos = FIRST_OFFSET;
    while pos + RECORD_LEN <= len {
        offsets.push(pos);
        pos += DATA_LEN + RECORD_LEN;
    }
    offsets
}

/// Writes `data` without its fingerprint records. Returns how many were removed,
/// or `None` (writing nothing) if the fingerprints are malformed.
pub fn remove_fingerprints(data: &[u8], out: &mut impl Write) -> io::Result<Option<usize>> {
    let Some(offsets) = fingerprint_offsets(data) else {
        return Ok(None);
    };
    let mut start = 0;
    for &pos in &offsets {
        out.write_all(&data[start..pos])?;
        start = pos + RECORD_LEN;
    }
    out.write_all(&data[start..])?;
    Ok(Some(offsets.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Python algorithm, literally, for comparison.
    fn reference(data: &[u8]) -> Option<Vec<u8>> {
        let mut d = data.to_vec();
        if !is_fingerprinted(&d) {
            return Some(d);
        }
        let mut offset = FIRST_OFFSET;
        while d.len() >= offset + RECORD_LEN {
            if d[offset..offset + 4] != SIGNATURE {
                return None;
            }
            d.drain(offset..offset + RECORD_LEN);
            offset += DATA_LEN;
        }
        Some(d)
    }

    fn sample(real_len: usize) -> (Vec<u8>, Vec<u8>) {
        // Interleave fingerprints into `real_len` bytes of data the way the device does.
        let real: Vec<u8> = (0..real_len).map(|i| (i % 251) as u8).collect();
        let mut out = real[..FIRST_OFFSET.min(real_len)].to_vec();
        let mut pos = FIRST_OFFSET;
        while pos < real_len {
            let mut rec = vec![0u8; RECORD_LEN];
            rec[..4].copy_from_slice(&SIGNATURE);
            out.extend_from_slice(&rec);
            let end = (pos + DATA_LEN).min(real_len);
            out.extend_from_slice(&real[pos..end]);
            pos = end;
        }
        (out, real)
    }

    #[test]
    fn matches_reference() {
        for real_len in [
            2048,
            DATA_LEN,
            DATA_LEN + 1024,
            DATA_LEN + 1034,
            3 * DATA_LEN + 4096,
            5 * DATA_LEN,
        ] {
            let (data, real) = sample(real_len);
            let mut out = Vec::new();
            let n = remove_fingerprints(&data, &mut out).unwrap().unwrap();
            assert_eq!(out, real, "len {real_len}");
            assert_eq!(Some(out), reference(&data), "len {real_len}");
            assert_eq!(n, (data.len() - real_len) / RECORD_LEN);
        }
    }

    #[test]
    fn plain_and_broken() {
        let plain = vec![0u8; 4096];
        let mut out = Vec::new();
        assert_eq!(remove_fingerprints(&plain, &mut out).unwrap(), Some(0));
        assert_eq!(out, plain);

        let (mut broken, _) = sample(3 * DATA_LEN);
        let second = FIRST_OFFSET + DATA_LEN + RECORD_LEN;
        broken[second] = 0;
        assert_eq!(fingerprint_offsets(&broken), None);
        assert_eq!(reference(&broken), None);
    }
}
