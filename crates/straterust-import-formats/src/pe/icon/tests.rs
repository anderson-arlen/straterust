use super::*;

fn fixture(image: &[u8], bits: u16) -> Vec<u8> {
    let mut data = vec![0; 0x600];
    let put16 = |data: &mut [u8], offset: usize, value: u16| {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes())
    };
    let put32 = |data: &mut [u8], offset: usize, value: u32| {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
    };
    data[..2].copy_from_slice(b"MZ");
    put32(&mut data, 0x3c, 0x80);
    data[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [(0x84, 0x14c), (0x86, 1), (0x94, 224), (0x98, 0x10b)] {
        put16(&mut data, offset, value);
    }
    put32(&mut data, 0x98 + 92, 16);
    put32(&mut data, 0x98 + 112, 0x1000);
    put32(&mut data, 0x98 + 116, 0x400);
    put32(&mut data, 0x178 + 12, 0x1000);
    put32(&mut data, 0x178 + 16, 0x400);
    put32(&mut data, 0x178 + 20, 0x200);
    let r = &mut data[0x200..];
    put16(r, 14, 2);
    for (offset, id, target) in [
        (16, 3, 0x80000020),
        (24, 14, 0x80000038),
        (48, 7, 0x80000050),
        (72, 1, 0x80000068),
        (96, 0x409, 128),
        (120, 0x409, 144),
    ] {
        put32(r, offset, id);
        put32(r, offset + 4, target);
    }
    for offset in [32, 56, 80, 104] {
        put16(r, offset + 14, 1);
    }
    put32(r, 128, 0x1000 + 192);
    put32(r, 132, image.len() as u32);
    put32(r, 144, 0x1000 + 160);
    put32(r, 148, 20);
    put16(r, 162, 1);
    put16(r, 164, 1);
    r[166] = 2;
    r[167] = 2;
    put16(r, 170, 1);
    put16(r, 172, bits);
    put32(r, 174, image.len() as u32);
    put16(r, 178, 7);
    r[192..192 + image.len()].copy_from_slice(image);
    data
}

fn dib() -> Vec<u8> {
    let mut data = vec![0; 120];
    for (offset, value) in [(0, 40u32), (4, 2), (8, 4)] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[12..14].copy_from_slice(&1u16.to_le_bytes());
    data[14..16].copy_from_slice(&4u16.to_le_bytes());
    data[44..48].copy_from_slice(&[0, 0, 255, 0]);
    data[48..52].copy_from_slice(&[0, 255, 0, 0]);
    data[104] = 0x21;
    data[108] = 0x12;
    data[116] = 0x80;
    data
}

#[test]
fn extracts_palette_and_bottom_up_pixels_with_the_original_transparency_mask() {
    let icon = windows_icon(&fixture(&dib(), 4)).unwrap();
    assert_eq!((icon.width, icon.height), (2, 2));
    assert_eq!(
        icon.rgba,
        [255, 0, 0, 0, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255]
    );
}

#[test]
fn supports_rgba_png_icons_and_rejects_corrupt_groups_or_truncated_resources() {
    let rgba = [
        255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    let mut image = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut image, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
    }
    let data = fixture(&image, 32);
    assert_eq!(windows_icon(&data).unwrap().rgba, rgba);
    for end in [0, 64, 256, 512, 700, 1024, 1535] {
        assert!(windows_icon(&data[..end]).is_err());
    }
    let mut bad = data.clone();
    bad[0x200 + 178] = 99;
    assert!(
        windows_icon(&bad)
            .unwrap_err()
            .to_string()
            .contains("missing image")
    );
    let mut bad = data;
    bad[0x200 + 166] = 3;
    assert!(
        windows_icon(&bad)
            .unwrap_err()
            .to_string()
            .contains("canvas mismatch")
    );
}
