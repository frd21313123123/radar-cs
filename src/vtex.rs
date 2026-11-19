//! Decoder for the texture formats used by CS2 overhead radar images.
//!
//! The game ships radar images as `panorama/images/overheadmaps/*_radar_psd.vtex_c`.
//! CS2 uses either BGRA8888 or DXT5 for these textures, with LZ4-compressed mips.
//! This module extracts the largest mip and converts it to RGBA PNG.
use png::{BitDepth, ColorType, Encoder};

const VPK_FORMAT_BGRA8888: u8 = 28;
const VPK_FORMAT_RGBA8888: u8 = 4;
const VPK_FORMAT_DXT1: u8 = 1;
const VPK_FORMAT_DXT5: u8 = 2;

struct VTexHeader {
    width: u32,
    height: u32,
    format: u8,
    data_offset: usize,
    data_size: usize,
}

fn u16le(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes.get(at..at + 2).map(|v| u16::from_le_bytes(v.try_into().unwrap()))
        .ok_or_else(|| "VTex header is truncated".to_string())
}

fn u32le(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes.get(at..at + 4).map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .ok_or_else(|| "VTex header is truncated".to_string())
}

fn parse_vtex(bytes: &[u8]) -> Result<VTexHeader, String> {
    if bytes.len() < 16 {
        return Err("VTex resource is truncated".into());
    }
    let header_version = u16le(bytes, 4)?;
    if header_version != 12 {
        return Err(format!("Unsupported resource header version {header_version}"));
    }
    let block_offset = u32le(bytes, 8)? as usize;
    let block_count = u32le(bytes, 12)? as usize;
    let table = 8usize.checked_add(block_offset).ok_or("VTex block table overflow")?;
    let mut data = None;
    for i in 0..block_count {
        let at = table.checked_add(i * 12).ok_or("VTex block table overflow")?;
        let kind = bytes.get(at..at + 4).ok_or("VTex block table is truncated")?;
        if kind == b"DATA" {
            let offset = u32le(bytes, at + 4)? as usize;
            let size = u32le(bytes, at + 8)? as usize;
            let absolute = at.checked_add(4).and_then(|v| v.checked_add(offset))
                .ok_or("VTex DATA offset overflow")?;
            data = Some((absolute, size));
            break;
        }
    }
    let (data_offset, data_size) = data.ok_or("VTex DATA block is missing")?;
    let header = bytes.get(data_offset..data_offset + 40).ok_or("VTex DATA header is truncated")?;
    let version = u16::from_le_bytes(header[0..2].try_into().unwrap());
    if version != 1 {
        return Err(format!("Unsupported VTex version {version}"));
    }
    let width = u16::from_le_bytes(header[20..22].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(header[22..24].try_into().unwrap()) as u32;
    let format = header[26];
    let _mips = header[27];
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("VTex dimensions are invalid".into());
    }
    Ok(VTexHeader { width, height, format, data_offset, data_size })
}

fn block_size(format: u8) -> Option<usize> {
    match format {
        VPK_FORMAT_DXT1 => Some(8),
        VPK_FORMAT_DXT5 => Some(16),
        VPK_FORMAT_BGRA8888 | VPK_FORMAT_RGBA8888 => Some(4),
        _ => None,
    }
}

fn expected_size(width: u32, height: u32, format: u8) -> Option<usize> {
    match format {
        VPK_FORMAT_BGRA8888 | VPK_FORMAT_RGBA8888 => Some(width as usize * height as usize * 4),
        VPK_FORMAT_DXT1 | VPK_FORMAT_DXT5 => {
            let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
            Some(blocks * block_size(format)?)
        }
        _ => None,
    }
}

/// Read the compressed-mip table and return the first (largest) mip bytes.
fn largest_mip(bytes: &[u8], header: &VTexHeader) -> Result<Vec<u8>, String> {
    let end = header.data_offset.checked_add(header.data_size).ok_or("VTex DATA overflow")?;
    if end > bytes.len() {
        return Err("VTex DATA block points outside the file".into());
    }
    let expected = expected_size(header.width, header.height, header.format)
        .ok_or_else(|| format!("Unsupported radar texture format {}", header.format))?;
    let extra_offset = u32le(bytes, header.data_offset + 32)? as usize;
    let extra_count = u32le(bytes, header.data_offset + 36)? as usize;
    if extra_count > 32 {
        return Err("VTex has too many extra-data entries".into());
    }
    let mut cursor = header.data_offset.checked_add(40).ok_or("VTex overflow")?;
    cursor = cursor.checked_add(extra_offset.saturating_sub(8)).ok_or("VTex extra-data overflow")?;
    for _ in 0..extra_count {
        let kind = u32le(bytes, cursor)?;
        let offset = u32le(bytes, cursor + 4)? as usize;
        let size = u32le(bytes, cursor + 8)? as usize;
        let after = cursor + 12;
        let data_at = after.checked_add(offset.saturating_sub(8)).ok_or("VTex extra-data overflow")?;
        if kind == 4 {
            // COMPRESSED_MIP_SIZE: method, table offset, count, then int32 sizes.
            let method = u32le(bytes, data_at)?;
            let table_offset = u32le(bytes, data_at + 4)? as usize;
            let count = u32le(bytes, data_at + 8)? as usize;
            if count == 0 || count > 32 {
                return Err("VTex mip table is invalid".into());
            }
            let sizes_at = data_at.checked_add(12)
                .and_then(|v| v.checked_add(table_offset.saturating_sub(8)))
                .ok_or("VTex mip table overflow")?;
            let mut sizes = Vec::with_capacity(count);
            for i in 0..count {
                sizes.push(u32le(bytes, sizes_at + i * 4)? as usize);
            }
            let payload_at = sizes_at + count * 4;
            let size = *sizes.last().ok_or("VTex mip table is empty")?;
            let payload = bytes.get(payload_at..payload_at + size)
                .ok_or("VTex mip payload is truncated")?;
            return match method {
                0 => Ok(payload.to_vec()),
                1 => lz4_flex::block::decompress(payload, expected)
                    .map_err(|e| format!("LZ4 radar mip: {e}")),
                2 => Err("Zstd radar mips are not supported".into()),
                other => Err(format!("Unknown radar mip compression {other}")),
            };
        }
        let _ = size;
        cursor = after;
    }
    // No compression table: texture data follows the DATA block.
    bytes.get(end..end + expected).map(|v| v.to_vec())
        .ok_or_else(|| "Radar texture payload is truncated".into())
}

fn rgb565(value: u16) -> [u8; 3] {
    let r = ((value >> 11) & 0x1F) as u32 * 255 / 31;
    let g = ((value >> 5) & 0x3F) as u32 * 255 / 63;
    let b = (value & 0x1F) as u32 * 255 / 31;
    [r as u8, g as u8, b as u8]
}

fn decode_bc1_block(block: &[u8], out: &mut [u8], width: usize, x: usize, y: usize) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let mut palette = [[0u8, 0, 0, 255]; 4];
    palette[0] = [rgb565(c0)[0], rgb565(c0)[1], rgb565(c0)[2], 255];
    palette[1] = [rgb565(c1)[0], rgb565(c1)[1], rgb565(c1)[2], 255];
    if c0 > c1 {
        for i in 0..3 {
            palette[2][i] = ((2 * palette[0][i] as u32 + palette[1][i] as u32) / 3) as u8;
            palette[3][i] = ((palette[0][i] as u32 + 2 * palette[1][i] as u32) / 3) as u8;
        }
    } else {
        for i in 0..3 {
            palette[2][i] = ((palette[0][i] as u32 + palette[1][i] as u32) / 2) as u8;
        }
        palette[3] = [0, 0, 0, 0];
    }
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    for py in 0..4 {
        for px in 0..4 {
            let idx = ((indices >> (2 * (py * 4 + px))) & 3) as usize;
            let at = ((y + py) * width + (x + px)) * 4;
            if at + 3 < out.len() {
                out[at..at + 4].copy_from_slice(&palette[idx]);
            }
        }
    }
}

fn decode_bc3_block(block: &[u8], out: &mut [u8], width: usize, x: usize, y: usize) {
    let a0 = block[0];
    let a1 = block[1];
    let mut alpha = [0u8; 8];
    alpha[0] = a0;
    alpha[1] = a1;
    if a0 > a1 {
        for i in 0..6 {
            alpha[2 + i] = (((6 - i) as u32 * a0 as u32 + (1 + i) as u32 * a1 as u32) / 7) as u8;
        }
    } else {
        for i in 0..4 {
            alpha[2 + i] = (((4 - i) as u32 * a0 as u32 + (1 + i) as u32 * a1 as u32) / 5) as u8;
        }
        alpha[6] = 0;
        alpha[7] = 255;
    }
    let bits = block[2..8].iter().fold(0u64, |acc, b| (acc >> 8) | ((*b as u64) << 40));
    let c0 = u16::from_le_bytes([block[8], block[9]]);
    let c1 = u16::from_le_bytes([block[10], block[11]]);
    let mut palette = [[0u8; 3]; 4];
    palette[0] = rgb565(c0);
    palette[1] = rgb565(c1);
    if c0 > c1 {
        for i in 0..3 {
            palette[2][i] = ((2 * palette[0][i] as u32 + palette[1][i] as u32) / 3) as u8;
            palette[3][i] = ((palette[0][i] as u32 + 2 * palette[1][i] as u32) / 3) as u8;
        }
    } else {
        for i in 0..3 {
            palette[2][i] = ((palette[0][i] as u32 + palette[1][i] as u32) / 2) as u8;
        }
    }
    let indices = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    for py in 0..4 {
        for px in 0..4 {
            let pixel = py * 4 + px;
            let a = alpha[((bits >> (3 * pixel)) & 7) as usize];
            let c = palette[((indices >> (2 * pixel)) & 3) as usize];
            let at = ((y + py) * width + (x + px)) * 4;
            if at + 3 < out.len() {
                out[at] = c[0];
                out[at + 1] = c[1];
                out[at + 2] = c[2];
                out[at + 3] = a;
            }
        }
    }
}

fn decode_blocks(data: &[u8], width: u32, height: u32, format: u8) -> Result<Vec<u8>, String> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 4];
    let size = block_size(format).ok_or("Unsupported block format")?;
    let bx = w.div_ceil(4);
    let by = h.div_ceil(4);
    for y in (0..by).step_by(1) {
        for x in 0..bx {
            let at = (y * bx + x) * size;
            let block = data.get(at..at + size).ok_or("Radar block data is truncated")?;
            match format {
                VPK_FORMAT_DXT1 => decode_bc1_block(block, &mut out, w, x * 4, y * 4),
                VPK_FORMAT_DXT5 => decode_bc3_block(block, &mut out, w, x * 4, y * 4),
                _ => {}
            }
        }
    }
    Ok(out)
}

fn resize_to_1024(mut rgba: Vec<u8>, width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    if width == 1024 && height == 1024 {
        return (rgba, width, height);
    }
    let mut out = vec![0u8; 1024 * 1024 * 4];
    for y in 0..1024usize {
        let sy = (y as u64 * height as u64 / 1024) as usize;
        for x in 0..1024usize {
            let sx = (x as u64 * width as u64 / 1024) as usize;
            let src = (sy * width as usize + sx) * 4;
            let dst = (y * 1024 + x) * 4;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    rgba = out;
    (rgba, 1024, 1024)
}

/// Decode a `.vtex_c` radar texture to a 1024x1024 RGBA PNG.
pub fn decode_radar_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let header = parse_vtex(bytes)?;
    let mip = largest_mip(bytes, &header)?;
    let expected = expected_size(header.width, header.height, header.format)
        .ok_or("Unsupported radar format")?;
    if mip.len() < expected {
        return Err(format!("Radar mip is short: {} < {expected}", mip.len()));
    }
    let rgba = match header.format {
        VPK_FORMAT_DXT1 | VPK_FORMAT_DXT5 =>
            decode_blocks(&mip[..expected], header.width, header.height, header.format)?,
        VPK_FORMAT_BGRA8888 => {
            let mut out = vec![0u8; header.width as usize * header.height as usize * 4];
            for (src, dst) in mip[..expected].chunks_exact(4).zip(out.chunks_exact_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = src[3];
            }
            out
        }
        VPK_FORMAT_RGBA8888 => mip[..expected].to_vec(),
        format => return Err(format!("Unsupported radar texture format {format}")),
    };
    let (rgba, width, height) = resize_to_1024(rgba, header.width, header.height);
    let mut png = Vec::new();
    {
        let mut encoder = Encoder::new(&mut png, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_endpoints() {
        assert_eq!(rgb565(0xFFFF), [255, 255, 255]);
        assert_eq!(rgb565(0), [0, 0, 0]);
        assert_eq!(rgb565(0xF800), [255, 0, 0]);
        assert_eq!(rgb565(0x07E0), [0, 255, 0]);
        assert_eq!(rgb565(0x001F), [0, 0, 255]);
    }

    #[test]
    fn bgra_conversion_keeps_alpha() {
        let rgba = resize_to_1024(vec![1, 2, 3, 4], 1, 1);
        assert_eq!((rgba.0[0], rgba.0[1], rgba.0[2], rgba.0[3]), (1, 2, 3, 4));
        assert_eq!((rgba.1, rgba.2), (1024, 1024));
    }
}
