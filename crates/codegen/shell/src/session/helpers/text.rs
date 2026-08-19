/// Return the largest valid UTF-8 character boundary at or before `index`.
#[inline]
pub(super) fn floor_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    if value.is_char_boundary(index) {
        return index;
    }
    let mut boundary = index;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}
