//! per-row delta (predictor) filtering for planar pixel data.
//!
//! before zstd, each row of each channel plane is replaced with the difference
//! between its bytes and a prediction (left, above, average, or paeth). smooth
//! gradients and photographic content turn into long runs of near-zero bytes that
//! zstd packs much tighter; the planar layout already gives zstd coherent
//! per-channel data, and the filter stacks on top of that.
//!
//! the filter is chosen per row by the png minimum-sum-of-absolute-differences
//! heuristic, so flat rows fall back to [`Filter::None`] and the filter can never
//! make a row meaningfully worse. each plane is single-channel, so the predictor's
//! "left" neighbour is the previous byte and "up" is the byte directly above.

/// filter type, stored as one byte at the start of each filtered row.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Filter {
    /// store the row unchanged.
    None = 0,
    /// predict each byte from the one to its left.
    Sub = 1,
    /// predict each byte from the one directly above.
    Up = 2,
    /// predict each byte from the average of left and above.
    Average = 3,
    /// predict each byte with the paeth predictor over left/above/above-left.
    Paeth = 4,
}

impl Filter {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Sub),
            2 => Some(Self::Up),
            3 => Some(Self::Average),
            4 => Some(Self::Paeth),
            _ => None,
        }
    }
}

/// the five filters in evaluation order.
const ALL_FILTERS: [Filter; 5] =
    [Filter::None, Filter::Sub, Filter::Up, Filter::Average, Filter::Paeth];

/// paeth predictor (png spec): pick whichever of left/above/above-left is closest
/// to the linear estimate `left + above - above_left`.
#[inline]
fn paeth(left: u8, above: u8, above_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(above) - i16::from(above_left);
    let distance_left = (estimate - i16::from(left)).abs();
    let distance_above = (estimate - i16::from(above)).abs();
    let distance_above_left = (estimate - i16::from(above_left)).abs();
    if distance_left <= distance_above && distance_left <= distance_above_left {
        left
    } else if distance_above <= distance_above_left {
        above
    } else {
        above_left
    }
}

#[inline]
fn average(left: u8, above: u8) -> u8 {
    ((u16::from(left) + u16::from(above)) / 2) as u8
}

/// extra bytes the filtered form adds over the raw planar buffer: one filter-type
/// byte per row per plane.
#[must_use]
pub fn overhead_bytes(height: usize, plane_count: usize) -> usize {
    height * plane_count
}

/// filter planar pixel data.
///
/// `planar` is `plane_count` channel planes laid out back to back, each
/// `width * height` bytes. the result is `plane_count * height * (1 + width)` bytes:
/// every row is prefixed with its chosen filter type. pair with [`unfilter_planes`].
#[must_use]
pub fn filter_planes(planar: &[u8], width: usize, height: usize, plane_count: usize) -> Vec<u8> {
    let plane_size = width * height;
    debug_assert_eq!(planar.len(), plane_size * plane_count);

    let filtered_row_len = width + 1;
    let mut out = vec![0u8; plane_count * height * filtered_row_len];
    // one scratch buffer per filter, reused across every row
    let mut candidates: [Vec<u8>; 5] = std::array::from_fn(|_| vec![0u8; width]);

    for plane in 0..plane_count {
        let plane_data = &planar[plane * plane_size..plane * plane_size + plane_size];
        for row in 0..height {
            let current = &plane_data[row * width..row * width + width];
            let above_row =
                if row == 0 { None } else { Some(&plane_data[(row - 1) * width..(row - 1) * width + width]) };

            for x in 0..width {
                let left = if x == 0 { 0 } else { current[x - 1] };
                let above = above_row.map_or(0, |a| a[x]);
                let above_left = if x == 0 { 0 } else { above_row.map_or(0, |a| a[x - 1]) };
                let value = current[x];
                candidates[Filter::None as usize][x] = value;
                candidates[Filter::Sub as usize][x] = value.wrapping_sub(left);
                candidates[Filter::Up as usize][x] = value.wrapping_sub(above);
                candidates[Filter::Average as usize][x] = value.wrapping_sub(average(left, above));
                candidates[Filter::Paeth as usize][x] = value.wrapping_sub(paeth(left, above, above_left));
            }

            // pick the filter whose output has the smallest sum of absolute signed
            // bytes — the standard png proxy for how well it will compress.
            let mut best = Filter::None;
            let mut best_score = u64::MAX;
            for filter in ALL_FILTERS {
                let score: u64 = candidates[filter as usize]
                    .iter()
                    .map(|&byte| u64::from((byte as i8).unsigned_abs()))
                    .sum();
                if score < best_score {
                    best_score = score;
                    best = filter;
                }
            }

            let row_start = (plane * height + row) * filtered_row_len;
            out[row_start] = best as u8;
            out[row_start + 1..row_start + 1 + width].copy_from_slice(&candidates[best as usize]);
        }
    }
    out
}

/// reverse [`filter_planes`].
///
/// `filtered` must be exactly `plane_count * height * (1 + width)` bytes. returns the
/// raw planar buffer (`plane_count * width * height` bytes), or `None` if the input
/// length is wrong or a filter-type byte is invalid.
#[must_use]
pub fn unfilter_planes(
    filtered: &[u8],
    width: usize,
    height: usize,
    plane_count: usize,
) -> Option<Vec<u8>> {
    let filtered_row_len = width + 1;
    if filtered.len() != plane_count * height * filtered_row_len {
        return None;
    }
    let plane_size = width * height;
    let mut out = vec![0u8; plane_count * plane_size];

    for plane in 0..plane_count {
        let plane_base = plane * plane_size;
        for row in 0..height {
            let row_start = (plane * height + row) * filtered_row_len;
            let filter = Filter::from_u8(filtered[row_start])?;
            let source = &filtered[row_start + 1..row_start + 1 + width];
            for x in 0..width {
                // left/above/above-left are already reconstructed in `out`
                let left = if x == 0 { 0 } else { out[plane_base + row * width + x - 1] };
                let above = if row == 0 { 0 } else { out[plane_base + (row - 1) * width + x] };
                let above_left =
                    if row == 0 || x == 0 { 0 } else { out[plane_base + (row - 1) * width + x - 1] };
                let predictor = match filter {
                    Filter::None => 0,
                    Filter::Sub => left,
                    Filter::Up => above,
                    Filter::Average => average(left, above),
                    Filter::Paeth => paeth(left, above, above_left),
                };
                out[plane_base + row * width + x] = source[x].wrapping_add(predictor);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(planar: &[u8], width: usize, height: usize, plane_count: usize) {
        let filtered = filter_planes(planar, width, height, plane_count);
        assert_eq!(filtered.len(), plane_count * height * (width + 1));
        let restored = unfilter_planes(&filtered, width, height, plane_count).expect("unfilter");
        assert_eq!(restored, planar, "roundtrip mismatch {width}x{height} x{plane_count}");
    }

    #[test]
    fn roundtrip_gradient() {
        // horizontal gradient per plane — Sub/Paeth should shine, but roundtrip must hold
        let (width, height, planes) = (17usize, 13usize, 4usize);
        let mut planar = vec![0u8; width * height * planes];
        for plane in 0..planes {
            for y in 0..height {
                for x in 0..width {
                    planar[plane * width * height + y * width + x] =
                        ((x * 7 + y * 3 + plane * 11) & 0xff) as u8;
                }
            }
        }
        roundtrip(&planar, width, height, planes);
    }

    #[test]
    fn roundtrip_flat() {
        let (width, height, planes) = (8, 8, 4);
        let planar = vec![200u8; width * height * planes];
        roundtrip(&planar, width, height, planes);
    }

    #[test]
    fn roundtrip_random_and_edges() {
        // deterministic pseudo-random content plus 1xN and Nx1 edge shapes
        let make = |width: usize, height: usize, planes: usize| {
            let mut data = vec![0u8; width * height * planes];
            let mut state = 0x1234_5678u32;
            for byte in &mut data {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte = (state >> 24) as u8;
            }
            data
        };
        for (width, height, planes) in [(31, 19, 4), (1, 20, 4), (20, 1, 4), (5, 5, 1), (3, 7, 2)] {
            roundtrip(&make(width, height, planes), width, height, planes);
        }
    }

    #[test]
    fn unfilter_rejects_bad_length() {
        assert!(unfilter_planes(&[0, 0, 0], 4, 4, 4).is_none());
    }
}
