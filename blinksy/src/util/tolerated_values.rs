use core::range::Range;

pub struct ToleratedValues<T> {
    pub range: Range<T>,
    pub ideal: T,
}

impl<T: PartialOrd> ToleratedValues<T> {
    pub fn new(ideal: T, range: Range<T>) -> Self {
        assert!(ideal < range.end, "ideal value must be in tolerated range");
        assert!(
            ideal >= range.start,
            "ideal value must be in tolerated range"
        );
        Self { range, ideal }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[should_panic]
    fn ideal_value_needs_to_be_in_range() {
        ToleratedValues::new(8, Range::from(1..5));
    }
}
