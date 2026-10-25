//! The stroke point-array codec (docs/format.md §4.2), ported from
//! `decode_stroke_values` in `kfxlib/yj_to_epub_notebook.py`.
//!
//! ```text
//! 01 01            signature
//! u32 LE           value count
//! ceil(n/2) bytes  4-bit instructions, high nibble first (odd n: last nibble is 0 padding)
//! operand bytes    consumed in order
//! ```
//! Each instruction gives an increment (immediate 0–3, or an operand of 1, 2
//! or 3 bytes; bit 3 negates it). Values are second-order deltas: the
//! increment changes the step, and the step changes the value.

use folio_kdf::ion::Diagnostics;

/// Decodes one point array. Like kfxlib, problems are logged and decoding goes
/// on where it can; `None` means there was too little data to try.
pub fn decode_stroke_values(data: &[u8], num_points: usize, name: &str, diag: &mut Diagnostics) -> Option<Vec<i64>> {
    if data.get(..2) != Some(&[1, 1][..]) {
        diag.error("stroke.signature", || format!("{name} signature is incorrect"));
    }
    let Some(count) = data.get(2..6) else {
        diag.error("stroke.short", || format!("{name} is too short for its header"));
        return None;
    };
    let num_vals = u32::from_le_bytes(count.try_into().unwrap()) as usize;
    if num_vals != num_points {
        diag.error("stroke.count", || {
            format!("{name} expected {num_points} values, found {num_vals}")
        });
    }
    let mut rest = &data[6..];
    if rest.len() * 2 < num_vals {
        diag.error("stroke.short", || {
            format!("{name} has too little data for {num_vals} values")
        });
        return None;
    }

    let instr_bytes = num_vals.div_ceil(2);
    let (instrs, operands) = rest.split_at(instr_bytes);
    if num_vals % 2 == 1 && instrs[instr_bytes - 1] & 0x0f != 0 {
        diag.error("stroke.padding", || format!("{name} has a non-zero padding nibble"));
    }
    rest = operands;

    let mut take = |n: usize| -> Option<&[u8]> {
        let (head, tail) = rest.split_at_checked(n)?;
        rest = tail;
        Some(head)
    };

    let mut vals = Vec::with_capacity(num_vals);
    let (mut value, mut change) = (0i64, 0i64);
    for i in 0..num_vals {
        let instr = (instrs[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 0x0f;
        let n = usize::from(instr & 3);
        let mut increment = if instr & 4 != 0 {
            n as i64
        } else {
            let Some(bytes) = take(n) else {
                diag.error("stroke.out_of_data", || format!("{name} ran out of data at value {i}"));
                break;
            };
            match n {
                0 => 0,
                1 => i64::from(bytes[0]),
                2 => i64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
                _ => {
                    diag.error("stroke.wide_operand", || {
                        format!("{name} has a 3-byte operand at value {i}")
                    });
                    i64::from(bytes[0]) + (i64::from(u16::from_le_bytes([bytes[1], bytes[2]])) << 8)
                }
            }
        };
        if instr & 8 != 0 {
            if increment == 0 {
                diag.error("stroke.negative_zero", || {
                    format!("{name} has a negative zero at value {i}")
                });
            }
            increment = -increment;
        }
        if i == 0 {
            change = 0;
            value = increment;
        } else {
            change = change.wrapping_add(increment);
            value = value.wrapping_add(change);
        }
        vals.push(value);
    }

    if !rest.is_empty() {
        diag.error("stroke.extra_data", || {
            format!("{name} has {} bytes of extra data", rest.len())
        });
    }
    Some(vals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(instrs: &[u8], operands: &[u8]) -> Vec<u8> {
        let mut out = vec![1, 1];
        out.extend_from_slice(&(instrs.len() as u32).to_le_bytes());
        for pair in instrs.chunks(2) {
            out.push(pair[0] << 4 | pair.get(1).copied().unwrap_or(0));
        }
        out.extend_from_slice(operands);
        out
    }

    #[test]
    fn second_order_deltas() {
        // first value 300 (2-byte operand), then increments +1, +1, 0, -2 (immediates/1-byte)
        let data = encode(&[0b0010, 0b0101, 0b0101, 0b0000, 0b1001], &[0x2c, 0x01, 0x02]);
        let mut d = Diagnostics::default();
        let v = decode_stroke_values(&data, 5, "x", &mut d).unwrap();
        // change: 0, 1, 2, 2, 0 → values 300, 301, 303, 305, 305
        assert_eq!(v, vec![300, 301, 303, 305, 305]);
        assert!(d.is_empty(), "{:?}", d.entries());
    }

    #[test]
    fn problems_are_reported_not_fatal() {
        let mut d = Diagnostics::default();
        assert_eq!(decode_stroke_values(&[1, 1, 9, 0, 0, 0], 9, "x", &mut d), None);
        let data = encode(&[0b0001, 0b0001], &[5]);
        let v = decode_stroke_values(&data, 3, "x", &mut d).unwrap();
        assert_eq!(v, vec![5]);
        assert_eq!(d.count("stroke.count"), 1);
        assert_eq!(d.count("stroke.out_of_data"), 1);
    }
}
