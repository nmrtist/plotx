#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub documents_painted: usize,
    pub full_documents_painted: usize,
    pub interactive_documents_painted: usize,
    pub line_series_visited: usize,
    /// Source points inspected when a line is dense enough to pool.
    pub line_source_points_scanned: usize,
    pub line_points_emitted: usize,
    /// Source points in x-visible slices visited by line rendering.
    pub line_source_points_visited: usize,
    pub line_points_submitted: usize,
    /// Source segments tested against a viewport while preparing contour LOD.
    pub contour_source_segments_scanned: usize,
    pub contour_segments_visited: usize,
    pub contour_segments_submitted: usize,
}

impl RenderStats {
    pub(crate) fn record_document(&mut self, detail: crate::screen::ScreenRenderDetail) {
        self.documents_painted += 1;
        match detail {
            crate::screen::ScreenRenderDetail::Full => self.full_documents_painted += 1,
            crate::screen::ScreenRenderDetail::Interactive => {
                self.interactive_documents_painted += 1;
            }
        }
    }

    pub(crate) fn record_line(&mut self, source: usize, submitted: usize, pooled: bool) {
        self.line_source_points_visited += source;
        self.line_points_submitted += submitted;
        self.line_points_emitted += submitted;
        if pooled {
            self.line_source_points_scanned += source;
        }
    }

    pub(crate) fn record_contour(&mut self, scanned: usize, visited: usize, submitted: usize) {
        self.contour_source_segments_scanned += scanned;
        self.contour_segments_visited += visited;
        self.contour_segments_submitted += submitted;
    }
}
