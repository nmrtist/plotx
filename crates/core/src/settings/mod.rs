mod io;
mod migrate;
mod model;
mod paths;

pub use io::load;
// Writing is deliberately crate-private: `PlotxApp::persist_settings` is the
// only flush path, so no caller can leave the live preferences and the file
// disagreeing.
pub(crate) use io::save;
#[cfg(test)]
pub(crate) use io::{load_from_paths, save_to_path};
pub use model::*;
pub use paths::{config_dir, data_local_dir};

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
mod tests;
