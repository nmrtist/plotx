use super::*;
use crate::actions::Action;
use crate::automation::{
    CAP_FIELD_NOISE_SCALE, CAP_FIELD_SIGNED, CapabilityId, ComponentRef, KIND_FIELD, ResourceRef,
    TargetRef,
};
use crate::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_FRACTION_OF_RANGE, CONTOUR_BASE_NOISE_FLOOR,
    CanvasDocument, Dataset, Nmr2DDataset, NmrDataset, ObjectFrame, PlotxApp, SeriesBinding,
    SeriesId,
};

#[path = "tests_fixture.rs"]
mod fixture;
use fixture::nmr1d_with;
pub(crate) use fixture::{contour_app, contour_app_with_plane, contour_spec};

#[path = "tests_addressing.rs"]
mod addressing_tests;
#[path = "tests_aggregate.rs"]
mod aggregate_tests;
#[path = "tests_capability.rs"]
mod capability_tests;
#[path = "tests_catalog.rs"]
mod catalog_tests;
#[path = "tests_editing.rs"]
mod editing_tests;

#[path = "provider_tests.rs"]
mod provider_tests;
