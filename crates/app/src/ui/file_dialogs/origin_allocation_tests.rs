use super::*;
use std::cell::Cell;
use std::io::{self, Read};
use std::rc::Rc;

const NON_POWER_OF_TWO_BYTES: usize = 5_003;

struct FiniteCountingReader {
    remaining: usize,
    consumed: Rc<Cell<usize>>,
}

impl FiniteCountingReader {
    fn new(remaining: usize, consumed: Rc<Cell<usize>>) -> Self {
        Self {
            remaining,
            consumed,
        }
    }
}

impl Read for FiniteCountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.remaining.min(buffer.len());
        buffer[..read].fill(0x5a);
        self.remaining -= read;
        self.consumed.set(self.consumed.get() + read);
        Ok(read)
    }
}

struct InfiniteCountingReader {
    consumed: Rc<Cell<usize>>,
}

impl Read for InfiniteCountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        buffer.fill(0x5a);
        self.consumed.set(self.consumed.get() + buffer.len());
        Ok(buffer.len())
    }
}

fn source_limits(max_input_bytes: usize, max_total_owned_bytes: usize) -> OriginLimits {
    OriginLimits {
        max_input_bytes,
        max_total_owned_bytes,
        ..OriginLimits::default()
    }
}

#[test]
fn unknown_non_power_of_two_source_rejects_conversion_peak_over_total_limit() {
    let total_limit = NON_POWER_OF_TWO_BYTES * 2 - 1;
    let consumed = Rc::new(Cell::new(0));
    let reader = FiniteCountingReader::new(NON_POWER_OF_TWO_BYTES, Rc::clone(&consumed));

    let result = read_bounded_origin(
        reader,
        None,
        source_limits(NON_POWER_OF_TWO_BYTES, total_limit),
    );
    let error = match result {
        Ok(bytes) => panic!(
            "a {}-byte Arc bypassed the {total_limit}-byte conversion-peak budget",
            bytes.len()
        ),
        Err(error) => error,
    };

    assert_eq!(consumed.get(), NON_POWER_OF_TWO_BYTES);
    assert!(error.contains("total owned bytes"), "{error}");
    assert!(error.contains(&total_limit.to_string()), "{error}");
}

#[test]
fn unknown_non_power_of_two_source_succeeds_at_exact_conversion_peak() {
    let total_limit = NON_POWER_OF_TWO_BYTES * 2;
    let consumed = Rc::new(Cell::new(0));
    let reader = FiniteCountingReader::new(NON_POWER_OF_TWO_BYTES, Rc::clone(&consumed));

    let bytes = read_bounded_origin(
        reader,
        None,
        source_limits(NON_POWER_OF_TWO_BYTES, total_limit),
    )
    .expect("an exact source allocation plus Arc copy must fit");

    assert_eq!(bytes.len(), NON_POWER_OF_TWO_BYTES);
    assert_eq!(consumed.get(), NON_POWER_OF_TWO_BYTES);
}

#[test]
fn unknown_source_consumes_only_the_one_byte_oversize_sentinel() {
    let input_limit = NON_POWER_OF_TWO_BYTES;
    let consumed = Rc::new(Cell::new(0));
    let reader = InfiniteCountingReader {
        consumed: Rc::clone(&consumed),
    };

    let error = read_bounded_origin(reader, None, source_limits(input_limit, input_limit * 3))
        .expect_err("an unknown-length source must stop at one byte over the input limit");

    assert_eq!(consumed.get(), input_limit + 1);
    assert_eq!(
        error,
        OriginError::LimitExceeded {
            resource: "input bytes",
            limit: input_limit,
            actual: input_limit + 1,
        }
        .to_string()
    );
}
