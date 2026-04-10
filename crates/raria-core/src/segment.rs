// raria-core: Segment planning and state tracking.
//
// This module handles splitting a download into segments (byte ranges)
// and tracking the state of each segment through its lifecycle.

use serde::{Deserialize, Serialize};

/// The lifecycle status of a single download segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentStatus {
    /// Not yet started.
    Pending,
    /// Actively downloading.
    Active,
    /// Successfully downloaded.
    Done,
    /// Failed (will retry).
    Failed,
}

/// Persistent state for a single download segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    /// Byte offset where this segment starts (inclusive).
    pub start: u64,
    /// Byte offset where this segment ends (exclusive).
    pub end: u64,
    /// Bytes downloaded within this segment so far.
    pub downloaded: u64,
    /// ETag for conditional resume.
    pub etag: Option<String>,
    /// Current segment status.
    pub status: SegmentStatus,
}

impl SegmentState {
    /// The total size of this segment in bytes.
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// The byte offset to resume from within this segment.
    pub fn resume_offset(&self) -> u64 {
        self.start + self.downloaded
    }

    /// How many bytes remain to download.
    pub fn remaining(&self) -> u64 {
        self.size().saturating_sub(self.downloaded)
    }

    /// Whether this segment is complete.
    pub fn is_done(&self) -> bool {
        self.status == SegmentStatus::Done || self.downloaded >= self.size()
    }
}

/// Plan how to split a file of `total_size` bytes into segments.
///
/// Returns a vector of `(start, end)` byte ranges. The ranges are
/// contiguous and non-overlapping: `[start, end)`.
///
/// If `total_size` is 0, returns a single empty segment `(0, 0)`.
/// If `num_segments` is 0, it is treated as 1.
pub fn plan_segments(total_size: u64, num_segments: u32) -> Vec<(u64, u64)> {
    let num_segments = num_segments.max(1) as u64;

    if total_size == 0 {
        return vec![(0, 0)];
    }

    let base_size = total_size / num_segments;
    let remainder = total_size % num_segments;
    let mut segments = Vec::with_capacity(num_segments as usize);
    let mut offset = 0u64;

    for i in 0..num_segments {
        let extra = if i < remainder { 1 } else { 0 };
        let seg_size = base_size + extra;
        segments.push((offset, offset + seg_size));
        offset += seg_size;
    }

    segments
}

/// Convert planned segment ranges into initial `SegmentState` objects.
pub fn init_segment_states(ranges: &[(u64, u64)]) -> Vec<SegmentState> {
    ranges
        .iter()
        .map(|&(start, end)| SegmentState {
            start,
            end,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SegmentState tests ────────────────────────────────────────────

    #[test]
    fn segment_state_size_calculation() {
        let seg = SegmentState {
            start: 100,
            end: 300,
            downloaded: 0,
            etag: None,
            status: SegmentStatus::Pending,
        };
        assert_eq!(seg.size(), 200);
    }

    #[test]
    fn segment_state_resume_offset() {
        let seg = SegmentState {
            start: 1000,
            end: 2000,
            downloaded: 500,
            etag: None,
            status: SegmentStatus::Active,
        };
        assert_eq!(seg.resume_offset(), 1500);
    }

    #[test]
    fn segment_state_remaining() {
        let seg = SegmentState {
            start: 0,
            end: 1000,
            downloaded: 750,
            etag: None,
            status: SegmentStatus::Active,
        };
        assert_eq!(seg.remaining(), 250);
    }

    #[test]
    fn segment_state_is_done_by_status() {
        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 50,
            etag: None,
            status: SegmentStatus::Done,
        };
        assert!(seg.is_done());
    }

    #[test]
    fn segment_state_is_done_by_bytes() {
        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 100,
            etag: None,
            status: SegmentStatus::Active,
        };
        assert!(seg.is_done());
    }

    #[test]
    fn segment_state_not_done() {
        let seg = SegmentState {
            start: 0,
            end: 100,
            downloaded: 50,
            etag: None,
            status: SegmentStatus::Active,
        };
        assert!(!seg.is_done());
    }

    #[test]
    fn segment_state_serde_roundtrips() {
        let seg = SegmentState {
            start: 0,
            end: 1024,
            downloaded: 512,
            etag: Some("W/\"abc123\"".into()),
            status: SegmentStatus::Active,
        };
        let json = serde_json::to_string(&seg).unwrap();
        let recovered: SegmentState = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.start, seg.start);
        assert_eq!(recovered.end, seg.end);
        assert_eq!(recovered.downloaded, seg.downloaded);
        assert_eq!(recovered.etag, seg.etag);
        assert_eq!(recovered.status, seg.status);
    }

    // ── plan_segments tests ───────────────────────────────────────────

    #[test]
    fn plan_segments_even_split() {
        let segs = plan_segments(1000, 4);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0], (0, 250));
        assert_eq!(segs[1], (250, 500));
        assert_eq!(segs[2], (500, 750));
        assert_eq!(segs[3], (750, 1000));
    }

    #[test]
    fn plan_segments_uneven_distributes_remainder() {
        let segs = plan_segments(10, 3);
        assert_eq!(segs.len(), 3);
        // 10 / 3 = 3 base, 1 remainder → first segment gets extra byte
        assert_eq!(segs[0], (0, 4));
        assert_eq!(segs[1], (4, 7));
        assert_eq!(segs[2], (7, 10));
    }

    #[test]
    fn plan_segments_single_segment() {
        let segs = plan_segments(5000, 1);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], (0, 5000));
    }

    #[test]
    fn plan_segments_zero_size() {
        let segs = plan_segments(0, 4);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], (0, 0));
    }

    #[test]
    fn plan_segments_zero_num_treated_as_one() {
        let segs = plan_segments(1000, 0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0], (0, 1000));
    }

    #[test]
    fn plan_segments_more_segments_than_bytes() {
        let segs = plan_segments(3, 10);
        assert_eq!(segs.len(), 10);
        // First 3 segments get 1 byte each, rest get 0.
        let total: u64 = segs.iter().map(|(s, e)| e - s).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn plan_segments_contiguous_coverage() {
        let segs = plan_segments(9999, 7);
        // Verify segments are contiguous.
        for i in 1..segs.len() {
            let prev = i - 1;
            assert_eq!(
                segs[i].0, segs[prev].1,
                "gap between segments {prev} and {i}"
            );
        }
        // Verify total coverage.
        assert_eq!(segs[0].0, 0);
        assert_eq!(segs.last().unwrap().1, 9999);
    }

    // ── init_segment_states tests ─────────────────────────────────────

    #[test]
    fn init_segment_states_creates_pending() {
        let ranges = vec![(0, 500), (500, 1000)];
        let states = init_segment_states(&ranges);
        assert_eq!(states.len(), 2);
        for s in &states {
            assert_eq!(s.downloaded, 0);
            assert_eq!(s.status, SegmentStatus::Pending);
            assert!(s.etag.is_none());
        }
        assert_eq!(states[0].start, 0);
        assert_eq!(states[0].end, 500);
        assert_eq!(states[1].start, 500);
        assert_eq!(states[1].end, 1000);
    }
}
