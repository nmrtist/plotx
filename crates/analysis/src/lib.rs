//! Analysis layer for plotx: detection, quantitative extraction, fitting, and
//! scientific parameter estimation over processed data.

pub mod alignment;
pub mod diffusion;
pub mod electrophysiology;
pub mod fit;
pub mod fit_model;
pub mod ilt;
pub mod integrate_2d;
pub mod lineshape;
pub mod models;
pub mod multiplet;
pub mod peaks;
mod pseudo2d_impl;
pub mod relaxation;
pub mod series;
mod series_reduce;
pub mod stack;
pub mod statistics;

pub use stack::SpectrumStack;
