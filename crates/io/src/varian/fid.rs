use crate::IoError;
use num_complex::Complex64;

const FILE_HEADER: usize = 32;
const BLOCK_HEADER: usize = 28;
const S_DATA: i16 = 0x1;
const S_SPEC: i16 = 0x2;
const S_32: i16 = 0x4;
const S_FLOAT: i16 = 0x8;
const S_COMPLEX: i16 = 0x10;
const S_HYPERCOMPLEX: i16 = 0x20;
const S_DDR: i16 = 0x80;
const S_SECND: i16 = 0x100;
const S_TRANSF: i16 = 0x200;
const S_3D: i16 = 0x400;
const SAMPLE_STATUS: i16 = S_32 | S_FLOAT;
const NB_HEADER_MASK: i32 = 0x0000f;
const NB_NI3: i32 = 0x10000;
const VERSION_FILE_ID_MASK: i16 = 0x07c0;
const VERSION_FID_FILE: i16 = 0x0040;

#[derive(Debug)]
pub(super) struct FidData {
    pub(super) traces: Vec<Vec<Complex64>>,
    pub(super) np: usize,
}

pub(super) fn parse(bytes: &[u8]) -> Result<FidData, IoError> {
    if bytes.len() < FILE_HEADER {
        return truncated(0, FILE_HEADER, bytes.len());
    }
    let nblocks = positive_i32(bytes, 0, "nblocks")?;
    let ntraces = positive_i32(bytes, 4, "ntraces")?;
    let np = positive_i32(bytes, 8, "np")?;
    let ebytes = positive_i32(bytes, 12, "ebytes")?;
    let tbytes = positive_i32(bytes, 16, "tbytes")?;
    let bbytes = positive_i32(bytes, 20, "bbytes")?;
    let version_id = i16::from_be_bytes(bytes[24..26].try_into().unwrap());
    let status = i16::from_be_bytes(bytes[26..28].try_into().unwrap());
    let raw_nbheaders = i32::from_be_bytes(bytes[28..32].try_into().unwrap());
    if raw_nbheaders & NB_NI3 != 0 {
        return Err(unsupported(
            "3D and 4D block-header layouts are not supported",
        ));
    }
    if raw_nbheaders & !(NB_NI3 | NB_HEADER_MASK) != 0 {
        return Err(invalid("nbheaders contains unknown layout flags"));
    }
    let nbheaders = usize::try_from(raw_nbheaders & NB_HEADER_MASK)
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| invalid("nbheaders must declare at least one block header"))?;
    if np % 2 != 0 {
        return Err(invalid("np must be a positive even number"));
    }
    if status & S_DATA == 0 || !is_complex_fid(status) {
        return Err(unsupported("fid is not complex time-domain data"));
    }
    if status & (S_SPEC | S_HYPERCOMPLEX) != 0 {
        return Err(unsupported(
            "processed spectra and hypercomplex payloads are not supported",
        ));
    }
    if status & (S_SECND | S_TRANSF | S_3D) != 0 {
        return Err(unsupported(
            "transformed, transposed, and 3D payloads are not supported",
        ));
    }
    let file_id = version_id & VERSION_FILE_ID_MASK;
    if file_id != 0 && file_id != VERSION_FID_FILE {
        return Err(unsupported(
            "the software-version header identifies a processed data file",
        ));
    }
    let sample = match (status & S_FLOAT != 0, status & S_32 != 0, ebytes) {
        (false, false, 2) => Sample::I16,
        (false, true, 4) => Sample::I32,
        (true, _, 4) => Sample::F32,
        _ => {
            return Err(unsupported(
                "status flags and ebytes do not describe int16, int32, or float32 samples",
            ));
        }
    };
    let expected_tbytes = np
        .checked_mul(ebytes)
        .ok_or_else(|| invalid("trace size overflow"))?;
    if tbytes != expected_tbytes {
        return Err(invalid("tbytes does not equal np * ebytes"));
    }
    let headers_bytes = nbheaders
        .checked_mul(BLOCK_HEADER)
        .ok_or_else(|| invalid("block header size overflow"))?;
    let trace_bytes = ntraces
        .checked_mul(tbytes)
        .ok_or_else(|| invalid("block trace size overflow"))?;
    let minimum_bbytes = headers_bytes
        .checked_add(trace_bytes)
        .ok_or_else(|| invalid("block size overflow"))?;
    if bbytes < minimum_bbytes {
        return Err(invalid("bbytes is smaller than its headers and traces"));
    }
    let declared = nblocks
        .checked_mul(bbytes)
        .and_then(|n| FILE_HEADER.checked_add(n))
        .ok_or_else(|| invalid("file size overflow"))?;
    if bytes.len() < declared {
        return truncated(0, declared, bytes.len());
    }
    if bytes.len() != declared {
        return Err(invalid(
            "file length does not match the declared block layout",
        ));
    }

    let total = nblocks
        .checked_mul(ntraces)
        .ok_or_else(|| invalid("trace count overflow"))?;
    let mut traces = Vec::with_capacity(total);
    for block in 0..nblocks {
        let block_at = FILE_HEADER + block * bbytes;
        let scale = i16::from_be_bytes(bytes[block_at..block_at + 2].try_into().unwrap());
        let block_status =
            i16::from_be_bytes(bytes[block_at + 2..block_at + 4].try_into().unwrap());
        if block_status & S_DATA == 0
            || (block_status & S_COMPLEX == 0 && status & S_DDR == 0)
            || block_status & S_SPEC != 0
        {
            return Err(invalid(
                "block header status is inconsistent with complex time-domain data",
            ));
        }
        if block_status & S_HYPERCOMPLEX != 0 {
            return Err(unsupported("hypercomplex block payloads are not supported"));
        }
        if block_status & SAMPLE_STATUS != status & SAMPLE_STATUS {
            return Err(invalid(
                "block sample type flags disagree with the file header",
            ));
        }
        let factor = 2.0_f64.powi(i32::from(scale));
        if !factor.is_finite() {
            return Err(invalid("block scale is out of range"));
        }
        for trace in 0..ntraces {
            let at = block_at + headers_bytes + trace * tbytes;
            let mut points = Vec::with_capacity(np / 2);
            for pair in 0..np / 2 {
                let real_at = at + pair * 2 * ebytes;
                points.push(Complex64::new(
                    sample.read(bytes, real_at) * factor,
                    sample.read(bytes, real_at + ebytes) * factor,
                ));
            }
            traces.push(points);
        }
    }
    Ok(FidData { traces, np })
}

fn is_complex_fid(status: i16) -> bool {
    status & (S_COMPLEX | S_DDR) != 0
}

#[derive(Clone, Copy)]
enum Sample {
    I16,
    I32,
    F32,
}
impl Sample {
    fn read(self, b: &[u8], at: usize) -> f64 {
        match self {
            Self::I16 => i16::from_be_bytes(b[at..at + 2].try_into().unwrap()) as f64,
            Self::I32 => i32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as f64,
            Self::F32 => f32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as f64,
        }
    }
}

fn positive_i32(bytes: &[u8], at: usize, name: &str) -> Result<usize, IoError> {
    let value = i32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    usize::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid(format!("{name} must be positive")))
}
fn invalid(message: impl Into<String>) -> IoError {
    IoError::InvalidVarian(message.into())
}
fn unsupported(message: impl Into<String>) -> IoError {
    IoError::UnsupportedVarian(message.into())
}
fn truncated<T>(offset: usize, needed: usize, have: usize) -> Result<T, IoError> {
    Err(IoError::Truncated {
        offset,
        needed,
        have,
    })
}
