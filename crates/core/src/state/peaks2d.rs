//! Persistent true-2D peak marks and their cross-diagonal relationships.

use serde::{Deserialize, Serialize};

use super::DatasetId;

/// Stable identity of one 2D peak within its owning dataset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Peak2DId(u64);

impl Peak2DId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Owner-scoped identity of a selected 2D peak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peak2DSelection {
    pub dataset: DatasetId,
    pub peak: Peak2DId,
}

impl Peak2DSelection {
    pub const fn new(dataset: DatasetId, peak: Peak2DId) -> Self {
        Self { dataset, peak }
    }

    pub fn in_dataset(self, dataset: DatasetId) -> Option<Peak2DId> {
        (self.dataset == dataset).then_some(self.peak)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Peak2DOrigin {
    #[default]
    Manual,
    SymmetryAudit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Peak2DReview {
    #[default]
    Unreviewed,
    Confirmed,
    Uncertain,
    PossibleArtifact,
}

impl Peak2DReview {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unreviewed => "Unreviewed",
            Self::Confirmed => "Confirmed",
            Self::Uncertain => "Uncertain",
            Self::PossibleArtifact => "Possible artifact",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peak2DMark {
    pub id: Peak2DId,
    pub f2: f64,
    pub f1: f64,
    pub intensity: f64,
    #[serde(default)]
    pub origin: Peak2DOrigin,
    #[serde(default)]
    pub review: Peak2DReview,
    /// The cross-diagonal mate when both peaks were accepted as one pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner: Option<Peak2DId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Peak2DSet {
    pub marks: Vec<Peak2DMark>,
    #[serde(default)]
    pub next_id: u64,
}

impl Peak2DSet {
    pub fn reseed(&mut self) {
        self.next_id = self
            .marks
            .iter()
            .map(|mark| mark.id.get().saturating_add(1))
            .max()
            .unwrap_or(0);
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for mark in &self.marks {
            if !ids.insert(mark.id) {
                return Err(format!("duplicate 2D peak id {}", mark.id.get()));
            }
            if !mark.f2.is_finite() || !mark.f1.is_finite() || !mark.intensity.is_finite() {
                return Err(format!(
                    "2D peak {} contains a non-finite value",
                    mark.id.get()
                ));
            }
        }
        for mark in &self.marks {
            let Some(partner) = mark.partner else {
                continue;
            };
            let Some(other) = self.marks.iter().find(|other| other.id == partner) else {
                return Err(format!(
                    "2D peak {} references missing partner {}",
                    mark.id.get(),
                    partner.get()
                ));
            };
            if other.partner != Some(mark.id) {
                return Err(format!(
                    "2D peak pair {} ↔ {} is not reciprocal",
                    mark.id.get(),
                    partner.get()
                ));
            }
        }
        Ok(())
    }

    pub fn mark(&self, id: Peak2DId) -> Option<&Peak2DMark> {
        self.marks.iter().find(|mark| mark.id == id)
    }

    pub fn find_near(
        &self,
        f2: f64,
        f1: f64,
        f2_tolerance: f64,
        f1_tolerance: f64,
    ) -> Option<Peak2DId> {
        self.marks
            .iter()
            .filter_map(|mark| {
                let dx = (mark.f2 - f2).abs() / f2_tolerance.max(f64::MIN_POSITIVE);
                let dy = (mark.f1 - f1).abs() / f1_tolerance.max(f64::MIN_POSITIVE);
                (dx <= 1.0 && dy <= 1.0).then_some((dx * dx + dy * dy, mark.id))
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, id)| id)
    }

    pub fn add_pair(
        &mut self,
        first: Peak2DPoint,
        second: Peak2DPoint,
        tolerances: [f64; 2],
        origin: Peak2DOrigin,
    ) -> Result<[Peak2DId; 2], String> {
        let first_id = match self.find_near(first.f2, first.f1, tolerances[0], tolerances[1]) {
            Some(id) => id,
            None => self.push(first, origin)?,
        };
        let second_id = match self.find_near(second.f2, second.f1, tolerances[0], tolerances[1]) {
            Some(id) => id,
            None => self.push(second, origin)?,
        };
        if first_id == second_id {
            return Err("The two symmetry positions resolve to the same peak.".to_owned());
        }
        for mark in &mut self.marks {
            if mark.id == first_id {
                mark.partner = Some(second_id);
            } else if mark.id == second_id {
                mark.partner = Some(first_id);
            } else if mark.partner == Some(first_id) || mark.partner == Some(second_id) {
                mark.partner = None;
            }
        }
        Ok([first_id, second_id])
    }

    pub fn add_single(
        &mut self,
        point: Peak2DPoint,
        tolerances: [f64; 2],
        origin: Peak2DOrigin,
        review: Peak2DReview,
    ) -> Result<Peak2DId, String> {
        if let Some(id) = self.find_near(point.f2, point.f1, tolerances[0], tolerances[1]) {
            if let Some(mark) = self.marks.iter_mut().find(|mark| mark.id == id) {
                mark.review = review;
            }
            return Ok(id);
        }
        let id = self.push(point, origin)?;
        if let Some(mark) = self.marks.iter_mut().find(|mark| mark.id == id) {
            mark.review = review;
        }
        Ok(id)
    }

    pub fn set_review(&mut self, id: Peak2DId, review: Peak2DReview) -> bool {
        let Some(mark) = self.marks.iter_mut().find(|mark| mark.id == id) else {
            return false;
        };
        mark.review = review;
        true
    }

    pub fn remove(&mut self, id: Peak2DId) -> bool {
        let before = self.marks.len();
        self.marks.retain(|mark| mark.id != id);
        if self.marks.len() == before {
            return false;
        }
        for mark in &mut self.marks {
            if mark.partner == Some(id) {
                mark.partner = None;
            }
        }
        true
    }

    fn push(&mut self, point: Peak2DPoint, origin: Peak2DOrigin) -> Result<Peak2DId, String> {
        if !point.f2.is_finite() || !point.f1.is_finite() || !point.intensity.is_finite() {
            return Err("A 2D peak position and intensity must be finite.".to_owned());
        }
        let id = self.allocate_id()?;
        self.marks.push(Peak2DMark {
            id,
            f2: point.f2,
            f1: point.f1,
            intensity: point.intensity,
            origin,
            review: Peak2DReview::Unreviewed,
            partner: None,
        });
        Ok(id)
    }

    fn allocate_id(&mut self) -> Result<Peak2DId, String> {
        loop {
            let id = Peak2DId::new(self.next_id);
            if !self.marks.iter().any(|mark| mark.id == id) {
                self.next_id = self
                    .next_id
                    .checked_add(1)
                    .ok_or_else(|| "The 2D peak id space is exhausted.".to_owned())?;
                return Ok(id);
            }
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or_else(|| "The 2D peak id space is exhausted.".to_owned())?;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak2DPoint {
    pub f2: f64,
    pub f1: f64,
    pub intensity: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(f2: f64, f1: f64) -> Peak2DPoint {
        Peak2DPoint {
            f2,
            f1,
            intensity: 10.0,
        }
    }

    #[test]
    fn pair_links_are_reciprocal_and_removal_unlinks_the_survivor() {
        let mut peaks = Peak2DSet::default();
        let [first, second] = peaks
            .add_pair(
                point(7.0, 3.0),
                point(3.0, 7.0),
                [0.01, 0.01],
                Peak2DOrigin::Manual,
            )
            .unwrap();
        assert_eq!(peaks.mark(first).unwrap().partner, Some(second));
        assert_eq!(peaks.mark(second).unwrap().partner, Some(first));
        assert!(peaks.validate().is_ok());

        assert!(peaks.remove(first));
        assert_eq!(peaks.mark(second).unwrap().partner, None);
    }

    #[test]
    fn adding_an_existing_point_updates_review_without_duplication() {
        let mut peaks = Peak2DSet::default();
        let first = peaks
            .add_single(
                point(7.0, 3.0),
                [0.01, 0.01],
                Peak2DOrigin::Manual,
                Peak2DReview::Uncertain,
            )
            .unwrap();
        let second = peaks
            .add_single(
                point(7.005, 3.0),
                [0.01, 0.01],
                Peak2DOrigin::SymmetryAudit,
                Peak2DReview::PossibleArtifact,
            )
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(peaks.marks.len(), 1);
        assert_eq!(
            peaks.mark(first).unwrap().review,
            Peak2DReview::PossibleArtifact
        );
    }

    #[test]
    fn selection_does_not_resolve_a_colliding_id_in_another_dataset() {
        let owner = DatasetId::new();
        let other = DatasetId::new();
        let id = Peak2DId::new(0);
        let selection = Peak2DSelection::new(owner, id);

        assert_eq!(selection.in_dataset(owner), Some(id));
        assert_eq!(selection.in_dataset(other), None);
    }
}
