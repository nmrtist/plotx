use crate::{DataError, Result, UnitSpec};
use std::collections::BTreeMap;

/// Controlled v1 mapping from user/UCUM spellings to immutable unit
/// contracts. A registry may be extended by a domain, but conflicting
/// redefinitions are rejected so unit meaning cannot depend on load order.
#[derive(Clone, Debug, Default)]
pub struct UnitRegistry {
    units: BTreeMap<String, UnitSpec>,
}

impl UnitRegistry {
    pub fn plotx_v1() -> Self {
        let mut registry = Self::default();
        registry
            .register_builtin("1", UnitSpec::dimensionless("1"))
            .register_builtin("%", scaled("ratio", dimensionless(), "1", "%", 0.01, 0.0))
            .register_builtin("ppm", UnitSpec::ppm())
            .register_builtin("s", si("time", "s", "s", 1.0))
            .register_builtin("ms", si("time", "s", "ms", 1e-3))
            .register_builtin("us", si("time", "s", "us", 1e-6))
            .register_builtin("µs", si("time", "s", "µs", 1e-6))
            .register_builtin("ns", si("time", "s", "ns", 1e-9))
            .register_builtin("min", si("time", "s", "min", 60.0))
            .register_builtin("h", si("time", "s", "h", 3_600.0))
            .register_builtin("Hz", frequency("Hz", 1.0))
            .register_builtin("kHz", frequency("kHz", 1e3))
            .register_builtin("MHz", frequency("MHz", 1e6))
            .register_builtin("m", si("length", "m", "m", 1.0))
            .register_builtin("cm", si("length", "m", "cm", 1e-2))
            .register_builtin("mm", si("length", "m", "mm", 1e-3))
            .register_builtin("um", si("length", "m", "um", 1e-6))
            .register_builtin("µm", si("length", "m", "µm", 1e-6))
            .register_builtin("V", si("electric_potential", "V", "V", 1.0))
            .register_builtin("mV", si("electric_potential", "V", "mV", 1e-3))
            .register_builtin("uV", si("electric_potential", "V", "uV", 1e-6))
            .register_builtin("µV", si("electric_potential", "V", "µV", 1e-6))
            .register_builtin("A", si("electric_current", "A", "A", 1.0))
            .register_builtin("mA", si("electric_current", "A", "mA", 1e-3))
            .register_builtin("uA", si("electric_current", "A", "uA", 1e-6))
            .register_builtin("µA", si("electric_current", "A", "µA", 1e-6))
            .register_builtin("nA", si("electric_current", "A", "nA", 1e-9))
            .register_builtin("pA", si("electric_current", "A", "pA", 1e-12))
            .register_builtin("T", si("magnetic_flux_density", "T", "T", 1.0))
            .register_builtin("mT", si("magnetic_flux_density", "T", "mT", 1e-3))
            .register_builtin("K", si("temperature", "K", "K", 1.0))
            .register_builtin(
                "Cel",
                scaled("temperature", dimension("K", 1), "K", "°C", 1.0, 273.15),
            )
            .register_builtin("°C", {
                let mut unit = scaled("temperature", dimension("K", 1), "K", "°C", 1.0, 273.15);
                unit.ucum = Some("Cel".into());
                unit
            })
            .register_builtin(
                "a.u.",
                domain_unit("a.u.", "space.nmrtist.plotx.unit.arbitrary"),
            );
        registry
    }

    pub fn resolve(&self, code: &str) -> Result<UnitSpec> {
        self.units.get(code).cloned().ok_or_else(|| {
            DataError::InvalidSchema(format!(
                "unit '{code}' is not registered; register a domain unit explicitly"
            ))
        })
    }

    pub fn compatible_codes(&self, source: &UnitSpec) -> Vec<&str> {
        self.units
            .iter()
            .filter(|(_, unit)| source.is_compatible_with(unit))
            .map(|(code, _)| code.as_str())
            .collect()
    }

    pub fn register(&mut self, code: impl Into<String>, unit: UnitSpec) -> Result<()> {
        let code = code.into();
        if code.trim().is_empty() {
            return Err(DataError::InvalidSchema("unit code is empty".into()));
        }
        unit.validate()?;
        match self.units.get(&code) {
            Some(existing) if existing != &unit => Err(DataError::InvalidSchema(format!(
                "unit code '{code}' already has a different contract"
            ))),
            Some(_) => Ok(()),
            None => {
                self.units.insert(code, unit);
                Ok(())
            }
        }
    }

    pub fn registered_domain_unit(
        &mut self,
        code: impl Into<String>,
        extension_id: impl Into<String>,
    ) -> Result<UnitSpec> {
        let code = code.into();
        let unit = domain_unit(&code, &extension_id.into());
        self.register(code, unit.clone())?;
        Ok(unit)
    }

    fn register_builtin(&mut self, code: &str, unit: UnitSpec) -> &mut Self {
        self.units.insert(code.into(), unit);
        self
    }
}

fn dimensionless() -> BTreeMap<String, i8> {
    BTreeMap::new()
}

fn dimension(base: &str, exponent: i8) -> BTreeMap<String, i8> {
    BTreeMap::from([(base.into(), exponent)])
}

fn si(quantity: &str, canonical: &str, display: &str, scale: f64) -> UnitSpec {
    scaled(
        quantity,
        dimension(canonical, 1),
        canonical,
        display,
        scale,
        0.0,
    )
}

fn frequency(display: &str, scale: f64) -> UnitSpec {
    scaled("frequency", dimension("s", -1), "Hz", display, scale, 0.0)
}

fn scaled(
    quantity: &str,
    dimension: BTreeMap<String, i8>,
    canonical: &str,
    display: &str,
    scale: f64,
    offset: f64,
) -> UnitSpec {
    UnitSpec {
        quantity: quantity.into(),
        dimension,
        canonical_unit: canonical.into(),
        display_unit: display.into(),
        scale,
        offset,
        ucum: Some(display.into()),
        extension_id: None,
    }
}

fn domain_unit(display: &str, extension_id: &str) -> UnitSpec {
    UnitSpec {
        quantity: "domain_quantity".into(),
        dimension: BTreeMap::new(),
        canonical_unit: display.into(),
        display_unit: display.into(),
        scale: 1.0,
        offset: 0.0,
        ucum: None,
        extension_id: Some(extension_id.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_units_convert_and_domain_units_require_exact_registration() {
        let mut registry = UnitRegistry::plotx_v1();
        let milliseconds = registry.resolve("ms").unwrap();
        let seconds = registry.resolve("s").unwrap();
        assert_eq!(milliseconds.convert_value(250.0, &seconds).unwrap(), 0.25);
        let celsius = registry.resolve("°C").unwrap();
        let kelvin = registry.resolve("K").unwrap();
        assert!((celsius.convert_value(25.0, &kelvin).unwrap() - 298.15).abs() < 1e-12);
        assert!(registry.resolve("furlong/fortnight").is_err());

        let custom = registry
            .registered_domain_unit("counts", "org.example.detector.counts.v1")
            .unwrap();
        assert_eq!(registry.resolve("counts").unwrap(), custom);
        assert!(!custom.is_compatible_with(&registry.resolve("a.u.").unwrap()));
        assert!(
            registry
                .register("counts", domain_unit("counts", "org.example.other.v1"))
                .is_err()
        );
    }
}
