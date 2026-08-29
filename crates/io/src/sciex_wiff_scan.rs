use crate::IoError;
use byteorder::{ByteOrder, LittleEndian};
use std::path::{Path, PathBuf};

pub(super) fn companion_path(path: &Path) -> Result<PathBuf, IoError> {
    let mut name = path
        .file_name()
        .ok_or_else(|| IoError::InvalidSciexWiff("the WIFF path has no filename".to_owned()))?
        .to_os_string();
    name.push(".scan");
    let mut companion = path.to_owned();
    companion.set_file_name(name);
    Ok(companion)
}

#[derive(Clone, Copy)]
pub(super) struct ScanPoint {
    pub raw_mz_bin: u32,
    pub raw_intensity: u32,
}

pub(super) fn decode_payload(payload: &[u8]) -> Vec<ScanPoint> {
    let mut points = Vec::new();
    let mut mz = 0_u32;
    let mut i = 0;
    while i < payload.len() {
        let b = payload[i];
        if b == 0xff && payload.get(i..i + 4) == Some(&[0xff; 4]) {
            break;
        }
        match b {
            0..=0x7f => {
                mz = mz.wrapping_add(b as u32);
                i += 1;
            }
            0x80..=0xfb => {
                points.push(ScanPoint {
                    raw_mz_bin: mz,
                    raw_intensity: (b & 0x7f) as u32,
                });
                i += 1;
            }
            0xfc => {
                if i + 1 >= payload.len() {
                    break;
                }
                points.push(ScanPoint {
                    raw_mz_bin: mz,
                    raw_intensity: payload[i + 1] as u32,
                });
                i += 2;
            }
            0xfd => {
                if i + 2 >= payload.len() {
                    break;
                }
                points.push(ScanPoint {
                    raw_mz_bin: mz,
                    raw_intensity: LittleEndian::read_u16(&payload[i + 1..i + 3]) as u32,
                });
                i += 3;
            }
            0xfe => {
                if i + 3 >= payload.len() {
                    break;
                }
                let value = payload[i + 1] as u32
                    | (payload[i + 2] as u32) << 8
                    | (payload[i + 3] as u32) << 16;
                points.push(ScanPoint {
                    raw_mz_bin: mz,
                    raw_intensity: value,
                });
                i += 4;
            }
            0xff => {
                if i + 4 >= payload.len() {
                    break;
                }
                points.push(ScanPoint {
                    raw_mz_bin: mz,
                    raw_intensity: LittleEndian::read_u32(&payload[i + 1..i + 5]),
                });
                i += 5;
            }
        }
    }
    points
}

pub(super) fn decode_scan_block(block: &[u8], absolute_base: usize) -> (Vec<ScanPoint>, usize) {
    let terminator = block.windows(4).position(|window| window == [0xff; 4]);
    let mut starts = vec![56.min(block.len())];
    if let Some(position) = terminator {
        starts.push(position.saturating_add(8).min(block.len()));
        starts.push(position.saturating_add(4).min(block.len()));
    }
    starts.push(0);
    let mut best = Vec::new();
    let mut best_start = 0;
    for start in starts {
        if start >= block.len() {
            continue;
        }
        let stop = block[start..]
            .windows(4)
            .position(|window| window == [0xff; 4])
            .map_or(block.len(), |position| start + position);
        let points = decode_payload(&block[start..stop]);
        if points.len() > best.len() {
            best = points;
            best_start = absolute_base + start;
        }
    }
    (best, best_start)
}
