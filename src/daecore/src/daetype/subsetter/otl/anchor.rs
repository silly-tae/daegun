use alloc::vec::Vec;
use crate::daecore::daetype::decoder::{read_i16_be, read_u16_be, write_i16_be, write_u16_be};

pub(crate) fn parse_anchor_point(buf: &[u8], off: usize) -> Option<u16> {
    (read_i16_be(buf, off)? == 2).then(|| read_u16_be(buf, off + 6)).flatten()
}

pub(crate) fn parse_anchor(buf: &[u8], off: usize) -> Option<(i16, i16)> {
    let format = read_i16_be(buf, off)?;
    if !(1..=3).contains(&format) { return None; }
    let x = read_i16_be(buf, off + 2)?;
    let y = read_i16_be(buf, off + 4)?;
    Some((x, y))
}

pub(crate) fn build_anchor(x: i16, y: i16) -> Vec<u8> {
    build_anchor_with_devices(x, y, None, None, None)
}

pub(crate) fn build_anchor_with_devices(
    x: i16,
    y: i16,
    point: Option<u16>,
    x_device: Option<&[u8]>,
    y_device: Option<&[u8]>,
) -> Vec<u8> {
    let format3 = x_device.is_some() || y_device.is_some();
    let format2 = !format3 && point.is_some();
    let header_len = if format3 { 10 } else if format2 { 8 } else { 6 };
    let mut out = vec![0u8; header_len];
    write_u16_be(&mut out, 0, if format3 { 3 } else if format2 { 2 } else { 1 });
    write_i16_be(&mut out, 2, x);
    write_i16_be(&mut out, 4, y);
    if format2 {
        write_u16_be(&mut out, 6, point.unwrap_or(0));
    }

    if format3 {
        for (slot, device) in [(6usize, x_device), (8, y_device)] {
            if let Some(bytes) = device {
                let target = out.len();
                let Ok(rel) = u16::try_from(target) else {
                    return build_anchor(x, y);
                };
                write_u16_be(&mut out, slot, rel);
                out.extend_from_slice(bytes);
            }
        }
    }
    out
}
