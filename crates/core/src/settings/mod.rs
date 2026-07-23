mod io;
mod migrate;
mod model;
mod paths;

pub use io::{load, save};
pub use model::*;
pub use paths::{config_dir, data_local_dir};

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

pub fn try_update(f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    let mut settings = load();
    f(&mut settings);
    save(&settings)
}

pub fn update(f: impl FnOnce(&mut Settings)) {
    if let Err(error) = try_update(f) {
        eprintln!("Could not save PlotX settings: {error}");
    }
}

#[cfg(test)]
mod tests;
