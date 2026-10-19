// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Clipboard formats, built as plain bytes; pure and tested.

/// `CF_HDROP`: a `DROPFILES` header and one wide path, double NUL-terminated.
/// Explorer pastes it as a copy of the file.
pub fn drop_files(path: &[u16]) -> Vec<u8> {
    // pFiles (offset of the list), pt.x, pt.y, fNC, fWide.
    let mut v: Vec<u8> = [20u32, 0, 0, 0, 1].iter().flat_map(|n| n.to_le_bytes()).collect();
    v.extend(path.iter().chain(&[0, 0]).flat_map(|c| c.to_le_bytes()));
    v
}

/// `CF_DIBV5`: a `BITMAPV5HEADER` for 32-bit sRGB pixels with straight
/// alpha, followed by the rows bottom-up. `bgra` holds them top-down.
/// Windows derives `CF_DIB` and `CF_BITMAP` from it for older programs.
pub fn dib_v5(w: u32, h: u32, bgra: &[u8]) -> Vec<u8> {
    const BI_BITFIELDS: u32 = 3;
    const LCS_SRGB: u32 = u32::from_be_bytes(*b"sRGB");
    const LCS_GM_IMAGES: u32 = 4;
    let stride = w as usize * 4;
    assert_eq!(bgra.len(), stride * h as usize);
    let mut v = Vec::with_capacity(124 + bgra.len());
    v.extend(124u32.to_le_bytes()); // bV5Size
    v.extend((w as i32).to_le_bytes());
    v.extend((h as i32).to_le_bytes()); // positive: bottom-up
    v.extend(1u16.to_le_bytes()); // planes
    v.extend(32u16.to_le_bytes()); // bits per pixel
    v.extend(BI_BITFIELDS.to_le_bytes());
    v.extend((bgra.len() as u32).to_le_bytes()); // image size
    v.extend([0u8; 16]); // resolution, colours used and important
    for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
        v.extend(mask.to_le_bytes()); // red, green, blue, alpha
    }
    v.extend(LCS_SRGB.to_le_bytes());
    v.extend([0u8; 36 + 12]); // endpoints, gamma
    v.extend(LCS_GM_IMAGES.to_le_bytes());
    v.extend([0u8; 12]); // profile data, size, reserved
    debug_assert_eq!(v.len(), 124);
    for row in bgra.chunks_exact(stride.max(1)).rev() {
        v.extend_from_slice(row);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_files_layout() {
        let v = drop_files(&[u16::from(b'C'), u16::from(b':')]);
        assert_eq!(&v[..4], &20u32.to_le_bytes());
        assert_eq!(&v[16..20], &1u32.to_le_bytes()); // wide
        assert_eq!(&v[20..], &[b'C', 0, b':', 0, 0, 0, 0, 0]);
    }

    #[test]
    fn dib_is_bottom_up_with_masks() {
        // 1×2: top pixel red, bottom pixel blue (BGRA).
        let v = dib_v5(1, 2, &[0, 0, 255, 255, 255, 0, 0, 255]);
        assert_eq!(v.len(), 124 + 8);
        assert_eq!(&v[..4], &124u32.to_le_bytes());
        assert_eq!(&v[8..12], &2i32.to_le_bytes());
        assert_eq!(&v[40..44], &0x00ff_0000u32.to_le_bytes()); // red mask
        assert_eq!(&v[56..60], b"BGRs"); // 'sRGB' as a little-endian DWORD
        assert_eq!(&v[124..128], &[255, 0, 0, 255]); // bottom row first: blue
        assert_eq!(&v[128..], &[0, 0, 255, 255]);
    }
}
