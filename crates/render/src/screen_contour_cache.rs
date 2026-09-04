use crate::screen::ScreenRenderDetail;
use crate::screen_lod::{ContourSegment, ScreenContourSegments, prepare_contour_lod};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

const CACHE_STATE_ID: &str = "plotx.screen_contour_lod_cache";
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ContourCacheKey {
    geometry_generation: u64,
    contour_index: usize,
    source_len: usize,
    viewport: [u64; 4],
}

impl ContourCacheKey {
    pub fn new(
        geometry_generation: u64,
        contour_index: usize,
        source_len: usize,
        viewport: [f64; 4],
    ) -> Self {
        let [x_min, x_max, y_min, y_max] = viewport;
        Self {
            geometry_generation,
            contour_index,
            source_len,
            viewport: [
                x_min.min(x_max).to_bits(),
                x_min.max(x_max).to_bits(),
                y_min.min(y_max).to_bits(),
                y_min.max(y_max).to_bits(),
            ],
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    lod: Arc<[usize]>,
    last_used: u64,
}

#[derive(Default)]
struct ContourCache {
    entries: HashMap<ContourCacheKey, CacheEntry>,
    clock: u64,
}

impl ContourCache {
    fn get(&mut self, key: ContourCacheKey) -> Option<Arc<[usize]>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = self.clock;
        Some(Arc::clone(&entry.lod))
    }

    fn insert(&mut self, key: ContourCacheKey, lod: Arc<[usize]>) {
        self.clock = self.clock.wrapping_add(1);
        if self.entries.len() >= MAX_CACHE_ENTRIES
            && !self.entries.contains_key(&key)
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key,
            CacheEntry {
                lod,
                last_used: self.clock,
            },
        );
    }
}

type SharedContourCache = Arc<Mutex<ContourCache>>;

pub(super) fn screen_contour_segments_cached<'a>(
    ctx: &egui::Context,
    cache_key: Option<ContourCacheKey>,
    segments: &'a [ContourSegment],
    detail: ScreenRenderDetail,
    viewport: [f64; 4],
    budget: usize,
) -> ScreenContourSegments<'a> {
    let Some(key) = cache_key else {
        return crate::screen_lod::screen_contour_segments(segments, detail, viewport, budget);
    };
    let cache = shared_cache(ctx);
    if let Some(lod) = lock_cache(&cache).get(key) {
        return match detail {
            ScreenRenderDetail::Full => ScreenContourSegments::full(segments, 0),
            ScreenRenderDetail::Interactive => {
                ScreenContourSegments::from_lod(segments, lod, budget, 0)
            }
        };
    }

    let lod = prepare_contour_lod(segments, viewport);
    lock_cache(&cache).insert(key, Arc::clone(&lod));
    match detail {
        ScreenRenderDetail::Full => ScreenContourSegments::full(segments, segments.len()),
        ScreenRenderDetail::Interactive => {
            ScreenContourSegments::from_lod(segments, lod, budget, segments.len())
        }
    }
}

fn shared_cache(ctx: &egui::Context) -> SharedContourCache {
    ctx.data_mut(|data| {
        let id = egui::Id::new(CACHE_STATE_ID);
        if let Some(cache) = data.get_temp::<SharedContourCache>(id) {
            return cache;
        }
        let cache = SharedContourCache::default();
        data.insert_temp(id, Arc::clone(&cache));
        cache
    })
}

fn lock_cache(cache: &SharedContourCache) -> MutexGuard<'_, ContourCache> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
