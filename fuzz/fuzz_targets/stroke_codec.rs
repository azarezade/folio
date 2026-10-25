//! Any input: the stroke codec must report problems, never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use folio_nmdl::codec::decode_stroke_values;

fuzz_target!(|data: &[u8]| {
    let mut diag = Default::default();
    // Use the encoded count as num_points half the time, so decoding gets past the count check.
    let count = data.get(2..6).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let num_points = if data.first().is_some_and(|b| b & 1 == 0) { count } else { count.wrapping_add(1) };
    let _ = decode_stroke_values(data, num_points, "fuzz", &mut diag);
});
