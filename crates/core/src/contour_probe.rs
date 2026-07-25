//! Test-only probes proving where derived contour work happens — which thread
//! runs marching squares, and whether resolving a warm cache still materializes
//! a full field payload.
//!
//! The counters are deliberately `thread_local`, and that is the whole point:
//! a `BuildContour` worker increments `MARCHING_SQUARES` on *its* thread, which
//! the test thread cannot see. "Zero on the calling thread" therefore means
//! exactly "the caller did not run marching squares itself". Promoting these to
//! an `AtomicUsize` (or any process-global counter) would make the assertions
//! count worker work as caller work and quietly destroy the property they
//! check, so keep them thread-local.
//!
//! Every marching-squares call site inside `plotx-core` must record here.
//! `ilt_figure_runs_marching_squares_on_the_calling_thread` is the positive
//! control: it fails if the instrumentation is ever dropped, so a "0 builds"
//! assertion can never pass merely because nothing counts.

use std::cell::Cell;

thread_local! {
    static MARCHING_SQUARES: Cell<usize> = const { Cell::new(0) };
    static QUEUED_CONTOUR_BUILDS: Cell<usize> = const { Cell::new(0) };
    static QUEUED_ESTIMATES: Cell<usize> = const { Cell::new(0) };
    static FIELD_PAYLOADS: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn reset() {
    MARCHING_SQUARES.with(|count| count.set(0));
    QUEUED_CONTOUR_BUILDS.with(|count| count.set(0));
    QUEUED_ESTIMATES.with(|count| count.set(0));
    FIELD_PAYLOADS.with(|count| count.set(0));
}

/// Record one marching-squares invocation on the current thread.
pub(crate) fn record_marching_squares() {
    MARCHING_SQUARES.with(|count| count.set(count.get().saturating_add(1)));
}

/// Marching-squares invocations made *on this thread* since [`reset`].
pub(crate) fn marching_squares_on_this_thread() -> usize {
    MARCHING_SQUARES.with(Cell::get)
}

pub(crate) fn record_queued_contour_build() {
    QUEUED_CONTOUR_BUILDS.with(|count| count.set(count.get().saturating_add(1)));
}

pub(crate) fn queued_contour_builds() -> usize {
    QUEUED_CONTOUR_BUILDS.with(Cell::get)
}

/// Record one `EstimateField` job actually handed to the workers. A result that
/// is cached — including a degenerate one — must never make this grow again.
pub(crate) fn record_queued_estimate() {
    QUEUED_ESTIMATES.with(|count| count.set(count.get().saturating_add(1)));
}

pub(crate) fn queued_estimates() -> usize {
    QUEUED_ESTIMATES.with(Cell::get)
}

/// Record one `Dataset::field_payload` call, i.e. one O(rows × cols) buffer.
pub(crate) fn record_field_payload() {
    FIELD_PAYLOADS.with(|count| count.set(count.get().saturating_add(1)));
}

pub(crate) fn field_payload_materializations() -> usize {
    FIELD_PAYLOADS.with(Cell::get)
}
