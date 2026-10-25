//! Small image helpers: screenshot thumbnails are downscaled once and cached.

use crate::app::Result;

/// Decodes a PNG and returns it as an 8-bit grey PNG `width` pixels wide
/// (box-filtered), or unchanged if it is already narrower.
pub fn downscale_png(data: &[u8], width: u32) -> Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let gray: Vec<u8> = buf[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|px| match channels {
            1 | 2 => px[0],
            _ => ((u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2])) / 3) as u8,
        })
        .collect();
    if w as u32 <= width {
        return Ok(folio_render::GrayImage {
            width: w as u32,
            height: h as u32,
            data: gray,
        }
        .to_png());
    }
    let nw = width as usize;
    let nh = ((h * nw) as f64 / w as f64).round().max(1.0) as usize;
    let mut out = vec![0u8; nw * nh];
    for oy in 0..nh {
        let (y0, y1) = (oy * h / nh, ((oy + 1) * h / nh).max(oy * h / nh + 1));
        for ox in 0..nw {
            let (x0, x1) = (ox * w / nw, ((ox + 1) * w / nw).max(ox * w / nw + 1));
            let mut sum = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += u32::from(gray[y * w + x]);
                }
            }
            out[oy * nw + ox] = (sum / ((y1 - y0) * (x1 - x0)) as u32) as u8;
        }
    }
    Ok(folio_render::GrayImage {
        width: nw as u32,
        height: nh as u32,
        data: out,
    }
    .to_png())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscales() {
        let img = folio_render::GrayImage {
            width: 1024,
            height: 512,
            data: vec![100; 1024 * 512],
        };
        let small = downscale_png(&img.to_png(), 256).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&small));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (256, 128));
    }
}
