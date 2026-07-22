use resvg::usvg::fontdb::Database;

/// Load platform fonts and make SVG's generic `sans-serif` family resolve on
/// systems that do not install fontdb's default Arial family (notably Linux).
pub(super) fn load_system_fonts(database: &mut Database) {
    database.load_system_fonts();

    const PREFERRED_SANS: &[&str] = &["Arial", "Liberation Sans", "DejaVu Sans", "Noto Sans"];
    let family = PREFERRED_SANS
        .iter()
        .find_map(|candidate| {
            database
                .faces()
                .flat_map(|face| &face.families)
                .find(|(family, _)| family.eq_ignore_ascii_case(candidate))
                .map(|(family, _)| family.clone())
        })
        .or_else(|| {
            database
                .faces()
                .find(|face| !face.monospaced)
                .or_else(|| database.faces().next())
                .and_then(|face| face.families.first())
                .map(|(family, _)| family.clone())
        });
    if let Some(family) = family {
        database.set_sans_serif_family(family);
    }
}
