//! JPEG and PNG to PDF image XObjects (ISO 32000-1 §8.9).
//!
//! JPEG data and opaque PNG data go into the PDF unchanged: DCTDecode, and
//! FlateDecode with the PNG predictor. Only PNGs with alpha are decoded, to
//! split the alpha channel into a soft mask.

use miniz_oxide::deflate::compress_to_vec_zlib;
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;

pub struct Image {
    /// Pixels.
    pub width: u32,
    pub height: u32,
    /// Image dictionary entries other than Type, Subtype, Width, Height and Length.
    pub dict: String,
    /// Stream data, encoded as `dict` says.
    pub data: Vec<u8>,
    /// Soft mask: dictionary entries and data, like `dict` and `data`.
    pub smask: Option<(String, Vec<u8>)>,
}

/// Decoded images larger than this are rejected.
const MAX_BYTES: usize = 256 << 20;

pub fn decode(bytes: &[u8]) -> Option<Image> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg(bytes)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)
    } else {
        None
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(i..i.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(i..i.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn jpeg(b: &[u8]) -> Option<Image> {
    let mut i = 2;
    let mut adobe = false;
    loop {
        if *b.get(i)? != 0xFF {
            return None;
        }
        let marker = *b.get(i + 1)?;
        i += 2;
        // Fill bytes and markers without a length.
        if marker == 0xFF {
            i -= 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            continue;
        }
        let len = usize::from(u16_at(b, i)?);
        let seg = b.get(i.checked_add(2)?..i.checked_add(len)?)?;
        match marker {
            0xEE if seg.starts_with(b"Adobe") => adobe = true,
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let bits = *seg.first()?;
                let height = u16_at(seg, 1)?;
                let width = u16_at(seg, 3)?;
                let (cs, decode) = match seg.get(5)? {
                    1 => ("/DeviceGray", ""),
                    3 => ("/DeviceRGB", ""),
                    // Adobe writes CMYK JPEGs inverted.
                    4 if adobe => ("/DeviceCMYK", " /Decode [1 0 1 0 1 0 1 0]"),
                    4 => ("/DeviceCMYK", ""),
                    _ => return None,
                };
                if width == 0 || height == 0 {
                    return None;
                }
                return Some(Image {
                    width: width.into(),
                    height: height.into(),
                    dict: format!(
                        "/ColorSpace {cs} /BitsPerComponent {bits} /Filter /DCTDecode{decode}"
                    ),
                    data: b.to_vec(),
                    smask: None,
                });
            }
            // Scan data or the end before a frame header.
            0xDA | 0xD9 => return None,
            _ => {}
        }
        i = i.checked_add(len)?;
    }
}

fn png(b: &[u8]) -> Option<Image> {
    let mut i = 8;
    let (mut ihdr, mut plte, mut idat) = (None, None, Vec::new());
    while let Some(len) = u32_at(b, i) {
        let len = usize::try_from(len).ok()?;
        let start = i.checked_add(8)?;
        let data = b.get(start..start.checked_add(len)?)?;
        match b.get(i + 4..start)? {
            b"IHDR" => ihdr = Some(data),
            b"PLTE" => plte = Some(data),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        i = start.checked_add(len)?.checked_add(4)?;
    }
    let h = ihdr?;
    let (width, height) = (u32_at(h, 0)?, u32_at(h, 4)?);
    let [depth, color, _, _, interlace] = *h.get(8..13)? else {
        return None;
    };
    // ponytail: Adam7-interlaced PNGs are skipped; decode them when someone needs it
    if width == 0 || height == 0 || interlace != 0 {
        return None;
    }
    let channels: usize = match (color, depth) {
        (0, 1 | 2 | 4 | 8 | 16) => 1,
        (3, 1 | 2 | 4 | 8) => 1,
        (2, 8 | 16) => 3,
        (4, 8 | 16) => 2,
        (6, 8 | 16) => 4,
        _ => return None,
    };
    let cs = match color {
        0 | 4 => "/DeviceGray".to_string(),
        2 | 6 => "/DeviceRGB".to_string(),
        _ => {
            let plte = plte.filter(|p| !p.is_empty() && p.len() % 3 == 0 && p.len() <= 768)?;
            let hex: String = plte.iter().map(|b| format!("{b:02X}")).collect();
            format!("[/Indexed /DeviceRGB {} <{hex}>]", plte.len() / 3 - 1)
        }
    };
    // ponytail: tRNS transparency is ignored; such images render opaque
    if color != 4 && color != 6 {
        return Some(Image {
            width,
            height,
            dict: format!(
                "/ColorSpace {cs} /BitsPerComponent {depth} /Filter /FlateDecode \
                 /DecodeParms << /Predictor 15 /Colors {channels} /BitsPerComponent {depth} /Columns {width} >>"
            ),
            data: idat,
            smask: None,
        });
    }

    let sample = usize::from(depth / 8);
    let bpp = channels * sample;
    let rows = usize::try_from(height).ok()?;
    let stride = usize::try_from(width).ok()?.checked_mul(bpp)?;
    let expected = stride.checked_add(1)?.checked_mul(rows)?;
    if expected > MAX_BYTES {
        return None;
    }
    let raw = decompress_to_vec_zlib_with_limit(&idat, expected).ok()?;
    let pixels = unfilter(&raw, stride, bpp, rows)?;
    let color_bytes = bpp - sample;
    let (mut rgb, mut alpha) = (
        Vec::with_capacity(pixels.len()),
        Vec::with_capacity(pixels.len() / channels),
    );
    for px in pixels.chunks_exact(bpp) {
        let (c, a) = px.split_at(color_bytes);
        rgb.extend_from_slice(c);
        alpha.extend_from_slice(a);
    }
    Some(Image {
        width,
        height,
        dict: format!("/ColorSpace {cs} /BitsPerComponent {depth} /Filter /FlateDecode"),
        data: compress_to_vec_zlib(&rgb, 6),
        smask: Some((
            format!("/ColorSpace /DeviceGray /BitsPerComponent {depth} /Filter /FlateDecode"),
            compress_to_vec_zlib(&alpha, 6),
        )),
    })
}

/// Reverses PNG row filters (PNG spec §9).
fn unfilter(raw: &[u8], stride: usize, bpp: usize, rows: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; stride.checked_mul(rows)?];
    let mut filtered = raw.chunks_exact(stride + 1);
    for r in 0..rows {
        let (&filter, src) = filtered.next()?.split_first()?;
        let (done, rest) = out.split_at_mut(r * stride);
        let prev = done
            .get(done.len().saturating_sub(stride)..)
            .filter(|_| r > 0);
        let row = rest.get_mut(..stride)?;
        for x in 0..stride {
            let a = if x >= bpp { row[x - bpp] } else { 0 };
            let b = prev.map_or(0, |p| p[x]);
            let c = if x >= bpp {
                prev.map_or(0, |p| p[x - bpp])
            } else {
                0
            };
            let v = src[x];
            row[x] = match filter {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
        }
    }
    Some(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Bytes of a base64 `data:` URI.
pub fn data_uri(src: &str) -> Option<Vec<u8>> {
    let rest = src.trim().strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let mut out = Vec::with_capacity(payload.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in payload.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6 | u32::from(v)) & 0xFF_FFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&[0; 4]);
        c
    }

    /// A PNG with the given color type and filtered rows.
    fn png_file(w: u32, h: u32, color: u8, rows: &[u8]) -> Vec<u8> {
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, color, 0, 0, 0]);
        let mut f = b"\x89PNG\r\n\x1a\n".to_vec();
        f.extend(chunk(b"IHDR", &ihdr));
        f.extend(chunk(b"IDAT", &compress_to_vec_zlib(rows, 6)));
        f.extend(chunk(b"IEND", &[]));
        f
    }

    #[test]
    fn opaque_png_passes_through() {
        let file = png_file(2, 1, 2, &[0, 255, 0, 0, 0, 255, 0]);
        let img = decode(&file).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert!(img.dict.contains("/Predictor 15 /Colors 3"));
        assert!(img.smask.is_none());
    }

    #[test]
    fn alpha_png_splits_mask() {
        // 2x2 RGBA; row 0 unfiltered, row 1 "Up" (copy of row 0 plus zero).
        let rows = [0, 10, 20, 30, 40, 50, 60, 70, 80, 2, 0, 0, 0, 1, 0, 0, 0, 0];
        let img = decode(&png_file(2, 2, 6, &rows)).unwrap();
        let rgb = miniz_oxide::inflate::decompress_to_vec_zlib(&img.data).unwrap();
        let (_, mask) = img.smask.unwrap();
        let alpha = miniz_oxide::inflate::decompress_to_vec_zlib(&mask).unwrap();
        assert_eq!(rgb, [10, 20, 30, 50, 60, 70, 10, 20, 30, 50, 60, 70]);
        assert_eq!(alpha, [40, 80, 41, 80]);
    }

    #[test]
    fn paeth_and_average_filters() {
        let rows = [0, 100, 50, 3, 10, 20, 4, 1, 1];
        let out = unfilter(&rows, 2, 1, 3).unwrap();
        assert_eq!(out[2..4], [10 + 50, 20 + (60 + 50) / 2]);
        assert_eq!(out[4..6], [61, 76]);
    }

    #[test]
    fn jpeg_header() {
        let mut f = vec![0xFF, 0xD8, 0xFF, 0xEE, 0, 7, b'A', b'd', b'o', b'b', b'e'];
        f.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8, 0, 3, 0, 5, 4, 1, 0x11, 0]);
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (5, 3));
        assert!(img.dict.contains("/DeviceCMYK") && img.dict.contains("/Decode"));
    }

    #[test]
    fn rejects_garbage() {
        let good = png_file(2, 2, 6, &[0; 18]);
        for end in 0..good.len() {
            decode(&good[..end]);
        }
        assert!(decode(&png_file(100_000, 100_000, 6, &[0; 18])).is_none());
        assert!(decode(b"\xFF\xD8\xFF\xC0\x00").is_none());
        assert!(decode(b"GIF89a").is_none());
    }

    #[test]
    fn base64() {
        assert_eq!(
            data_uri("data:image/png;base64,aGVsbG8=").unwrap(),
            b"hello"
        );
        assert_eq!(data_uri("data:;base64,aGVs\nbG8h").unwrap(), b"hello!");
        assert!(data_uri("data:image/png,raw").is_none());
        assert!(data_uri("x.png").is_none());
    }
}
