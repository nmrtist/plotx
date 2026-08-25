use super::{Endian, Reader};
use crate::jeol::ruler::{ascii_trim, prefix_exponent};
use std::collections::HashMap;

pub(super) struct Params {
    pub(super) f64s: HashMap<String, f64>,
    /// SI-normalized numeric values (raw value with its scaler prefix folded in).
    si: HashMap<String, f64>,
    pub(super) strings: HashMap<String, String>,
}

impl Params {
    const SCALER: usize = 0x06;
    const VALUE: usize = 0x10;
    const VALUE_TYPE: usize = 0x20;
    const NAME: usize = 0x24;
    const NAME_LEN: usize = 28;

    pub(super) fn empty() -> Self {
        Self {
            f64s: HashMap::new(),
            si: HashMap::new(),
            strings: HashMap::new(),
        }
    }

    // List header at `at` (body endianness): record_size u32, low_index u32,
    // high_index u32, total_size u32; then fixed-size records.
    pub(super) fn parse(bytes: &[u8], at: usize, endian: Endian) -> Self {
        let r = Reader { bytes, endian };
        if at + 16 > bytes.len() {
            return Self::empty();
        }
        let rec_size = r.u32(at) as usize;
        let high = r.u32(at + 8) as usize;
        if !(Self::NAME + Self::NAME_LEN..=4096).contains(&rec_size) {
            return Self::empty();
        }
        let count = high.saturating_add(1).min(4096);
        let base = at + 16;
        let mut out = Self::empty();
        for i in 0..count {
            let rec = base + i * rec_size;
            if rec + rec_size > bytes.len() {
                break;
            }
            let name = ascii_trim(&bytes[rec + Self::NAME..rec + Self::NAME + Self::NAME_LEN]);
            if name.is_empty() {
                continue;
            }
            match r.u32(rec + Self::VALUE_TYPE) {
                2 => {
                    let raw = r.f64(rec + Self::VALUE);
                    let si = raw * 10f64.powi(prefix_exponent(bytes[rec + Self::SCALER]));
                    out.si.insert(name.clone(), si);
                    out.f64s.insert(name, raw);
                }
                0 => {
                    let value = ascii_trim(&bytes[rec + Self::VALUE..rec + Self::VALUE + 16]);
                    if !value.is_empty() {
                        out.strings.insert(name, value);
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub(super) fn f64(&self, name: &str) -> Option<f64> {
        self.f64s
            .get(name)
            .copied()
            .filter(|value| value.is_finite())
    }

    pub(super) fn string(&self, name: &str) -> Option<String> {
        self.strings.get(name).cloned()
    }

    pub(super) fn si(&self, name: &str) -> Option<f64> {
        self.numeric_ci(name, &self.si)
    }

    fn numeric_ci(&self, name: &str, values: &HashMap<String, f64>) -> Option<f64> {
        values
            .get(name)
            .copied()
            .or_else(|| {
                values
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| *value)
            })
            .filter(|value| value.is_finite())
    }

    pub(super) fn string_ci(&self, name: &str) -> Option<String> {
        self.strings.get(name).cloned().or_else(|| {
            self.strings
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
        })
    }
}
