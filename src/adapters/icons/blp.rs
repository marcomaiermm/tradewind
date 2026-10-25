//! Bounded BLP2 decoding: palette, BC1/2/3, and BGRA. No external mipmap files.
use super::Icon;
use anyhow::{Result, bail, ensure};

fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| anyhow::anyhow!("Truncated BLP"))?
            .try_into()?,
    ))
}
pub(super) fn decode(data: &[u8]) -> Result<Icon> {
    ensure!(
        data.len() >= 148 && data.len() <= 4 * 1024 * 1024 && &data[..4] == b"BLP2",
        "Unsupported BLP"
    );
    ensure!(word(data, 4)? == 1, "Unsupported BLP content");
    let (width, height) = (word(data, 12)?, word(data, 16)?);
    ensure!(
        (1..=1024).contains(&width) && (1..=1024).contains(&height),
        "Invalid BLP dimensions"
    );
    let offset = word(data, 20)? as usize;
    let size = word(data, 84)? as usize;
    ensure!(offset >= 148, "Invalid BLP offset");
    let pixels = (width * height) as usize;
    let input = data
        .get(
            offset
                ..offset
                    .checked_add(size)
                    .ok_or_else(|| anyhow::anyhow!("BLP overflow"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("Truncated BLP mipmap"))?;
    let mut rgba = vec![0u8; pixels * 4];
    match data[8] {
        1 => {
            let alpha_bits = data[9] as usize;
            ensure!(
                [0, 1, 4, 8].contains(&alpha_bits),
                "Invalid BLP alpha depth"
            );
            ensure!(
                offset >= 1172
                    && data.len() >= 1172
                    && input.len() >= pixels + (pixels * alpha_bits).div_ceil(8),
                "Truncated BLP palette"
            );
            for (i, out) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let color = 148 + input[i] as usize * 4;
                let alpha = match alpha_bits {
                    0 => 255,
                    8 => input[pixels + i],
                    bits => {
                        ((input[pixels + i * bits / 8] >> (i * bits % 8)) & ((1 << bits) - 1))
                            * (255 / ((1 << bits) - 1))
                    }
                };
                out.copy_from_slice(&[data[color + 2], data[color + 1], data[color], alpha]);
            }
        }
        2 => {
            let format = match data[10] {
                0 => texpresso::Format::Bc1,
                1 => texpresso::Format::Bc2,
                7 => texpresso::Format::Bc3,
                _ => bail!("Unsupported BLP block compression"),
            };
            ensure!(
                input.len() == format.compressed_size(width as usize, height as usize),
                "Invalid BLP block size"
            );
            format.decompress(input, width as usize, height as usize, &mut rgba);
        }
        3 => {
            ensure!(input.len() == rgba.len(), "Invalid BLP pixel size");
            for (out, color) in rgba
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(input.as_chunks::<4>().0)
            {
                out.copy_from_slice(&[color[2], color[1], color[0], color[3]]);
            }
        }
        _ => bail!("Unsupported BLP encoding"),
    }
    // Icons are normally 64px. Bound both the persistent and display cache even for larger textures.
    let scale = (width.max(height) as f32 / 64.0).max(1.0);
    let target_width = (width as f32 / scale).round().max(1.0) as u32;
    let target_height = (height as f32 / scale).round().max(1.0) as u32;
    let mut resized = Vec::with_capacity((target_width * target_height * 4) as usize);
    for y in 0..target_height {
        for x in 0..target_width {
            let i = ((y * height / target_height * width + x * width / target_width) * 4) as usize;
            resized.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    Ok(Icon {
        width: target_width,
        height: target_height,
        rgba: resized.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(encoding: u8, alpha: u8, kind: u8, offset: u32, size: u32) -> Vec<u8> {
        let mut bytes = vec![0; offset as usize];
        bytes[..4].copy_from_slice(b"BLP2");
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&[encoding, alpha, kind, 0]);
        for p in [12, 16] {
            bytes[p..p + 4].copy_from_slice(&4u32.to_le_bytes());
        }
        bytes[20..24].copy_from_slice(&offset.to_le_bytes());
        bytes[84..88].copy_from_slice(&size.to_le_bytes());
        bytes
    }
    #[test]
    fn fixed_palette_alpha_bgra_and_bc1() {
        let mut palette = header(1, 4, 0, 1172, 24);
        palette[148..152].copy_from_slice(&[30, 20, 10, 0]);
        palette.extend([0; 16]);
        palette.extend([0xf0; 8]);
        let icon = decode(&palette).unwrap();
        assert_eq!(&icon.rgba[..8], &[10, 20, 30, 0, 10, 20, 30, 255]);
        let mut bgra = header(3, 8, 0, 148, 64);
        bgra.extend([30, 20, 10, 40].repeat(16));
        assert_eq!(&decode(&bgra).unwrap().rgba[..4], &[10, 20, 30, 40]);
        let mut bc1 = header(2, 0, 0, 148, 8);
        bc1.extend([0, 248, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&decode(&bc1).unwrap().rgba[..4], &[255, 0, 0, 255]);
        let mut bc2 = header(2, 4, 1, 148, 16);
        bc2.extend([0x88; 8]);
        bc2.extend([0, 248, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&decode(&bc2).unwrap().rgba[..4], &[255, 0, 0, 136]);
        let mut bc3 = header(2, 8, 7, 148, 16);
        bc3.extend([128, 64, 0, 0, 0, 0, 0, 0]);
        bc3.extend([0, 248, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&decode(&bc3).unwrap().rgba[..4], &[255, 0, 0, 128]);
        bgra.pop();
        assert!(decode(&bgra).is_err());
        bc1[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bc1).is_err());
    }
}
