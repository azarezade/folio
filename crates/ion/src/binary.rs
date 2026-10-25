//! Ion 1.0 binary primitives: VarUInt, VarInt, UInt and Int fields.
//!
//! Ported from `kfxlib/ion_binary.py` (`deserialize_vluint` and friends),
//! including its strictness: padded encodings are errors.

use crate::{Error, Result};

/// A forward-only reader over a byte slice that hands out borrowed sub-slices.
#[derive(Clone, Debug)]
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Cursor { data }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        self.data
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.data.first().copied()
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        let (&b, rest) = self.data.split_first().ok_or(Error::Eof)?;
        self.data = rest;
        Ok(b)
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.data.len() {
            return Err(Error::Eof);
        }
        let (head, rest) = self.data.split_at(n);
        self.data = rest;
        Ok(head)
    }

    /// VarUInt: 7 bits per byte, big-endian, high bit marks the last byte.
    pub(crate) fn varuint(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        loop {
            let b = self.u8()?;
            value = (value << 7) | u64::from(b & 0x7f);
            if b & 0x80 != 0 {
                return Ok(value);
            }
            if value == 0 {
                return Err(Error::invalid("VarUInt padded with 0x00"));
            }
            if value > 0x7f_ffff_ffff_ffff {
                return Err(Error::invalid("VarUInt too large, missing terminator"));
            }
        }
    }

    pub(crate) fn varuint_usize(&mut self) -> Result<usize> {
        usize::try_from(self.varuint()?).map_err(|_| Error::invalid("length too large"))
    }

    /// VarInt: like VarUInt, with the sign in bit 6 of the first byte.
    /// `Ok(None)` is negative zero, which only timestamps allow.
    pub(crate) fn varint(&mut self, allow_minus_zero: bool) -> Result<Option<i64>> {
        let first = self.u8()?;
        let mut bytes = [0u8; 10];
        let mut len = 0;
        let mut b = first & 0xbf;
        if b != 0 {
            bytes[0] = b;
            len = 1;
        }
        while b & 0x80 == 0 {
            b = self.u8()?;
            if len == bytes.len() {
                // kfxlib would reject this as padded or too large
                return Err(Error::invalid("VarInt too long"));
            }
            bytes[len] = b;
            len += 1;
        }
        let mut inner = Cursor::new(&bytes[..len]);
        let magnitude = inner.varuint()? as i64; // at most 62 bits
        if first & 0x40 != 0 {
            if magnitude != 0 {
                Ok(Some(-magnitude))
            } else if allow_minus_zero {
                Ok(None)
            } else {
                Err(Error::invalid("unexpected VarInt -0"))
            }
        } else {
            Ok(Some(magnitude))
        }
    }
}

/// UInt field: big-endian magnitude of at most 8 bytes, no leading zero byte.
pub(crate) fn uint(data: &[u8]) -> Result<u64> {
    if data.first() == Some(&0) {
        return Err(Error::invalid("UInt padded with 0x00"));
    }
    if data.len() > 8 {
        return Err(Error::invalid("UInt longer than 8 bytes"));
    }
    Ok(data.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

/// Int field (sign-magnitude, sign in the top bit); empty means 0.
pub(crate) fn signed_int(data: &[u8]) -> Result<i128> {
    let Some((&first, rest)) = data.split_first() else {
        return Ok(0);
    };
    let negative = first & 0x80 != 0;
    let mut bytes = Vec::with_capacity(data.len());
    bytes.push(first & 0x7f);
    bytes.extend_from_slice(rest);
    let start = bytes.len().saturating_sub(8);
    if bytes[..start].iter().any(|&b| b != 0) {
        return Err(Error::invalid("Int longer than 8 bytes"));
    }
    let magnitude = bytes[start..].iter().fold(0i128, |acc, &b| (acc << 8) | i128::from(b));
    Ok(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varuint() {
        assert_eq!(Cursor::new(&[0x81]).varuint(), Ok(1));
        assert_eq!(Cursor::new(&[0x01, 0x80]).varuint(), Ok(128));
        assert_eq!(Cursor::new(&[0x7f, 0xff]).varuint(), Ok(0x3fff));
        assert!(Cursor::new(&[0x00, 0x81]).varuint().is_err());
        assert_eq!(Cursor::new(&[0x01]).varuint(), Err(Error::Eof));
    }

    #[test]
    fn varint() {
        assert_eq!(Cursor::new(&[0x81]).varint(false), Ok(Some(1)));
        assert_eq!(Cursor::new(&[0xc1]).varint(false), Ok(Some(-1)));
        assert_eq!(Cursor::new(&[0xc0]).varint(true), Ok(None));
        assert!(Cursor::new(&[0xc0]).varint(false).is_err());
        assert_eq!(Cursor::new(&[0x41, 0x80]).varint(false), Ok(Some(-128)));
        assert_eq!(Cursor::new(&[0x00, 0x81]).varint(false), Ok(Some(1)));
    }

    #[test]
    fn ints() {
        assert_eq!(uint(&[]), Ok(0));
        assert_eq!(uint(&[0x01, 0x00]), Ok(256));
        assert!(uint(&[0x00, 0x01]).is_err());
        assert_eq!(signed_int(&[0x81]), Ok(-1));
        assert_eq!(signed_int(&[0x00, 0xff]), Ok(255));
        assert_eq!(signed_int(&[]), Ok(0));
    }
}
