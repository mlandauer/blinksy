use core::{
    ops::{Add, Sub},
    range::RangeInclusive,
};

pub struct ToleratedValues<T> {
    pub range: RangeInclusive<T>,
    pub ideal: T,
}

impl<T> ToleratedValues<T>
where
    T: PartialOrd + Add<Output = T> + Sub<Output = T> + Copy,
{
    pub fn range(ideal: T, range: RangeInclusive<T>) -> Self {
        assert!(
            (ideal >= range.start) && (ideal <= range.last),
            "ideal value must be in tolerated range"
        );
        Self { range, ideal }
    }

    pub fn new(ideal: T, tolerance: T) -> Self {
        Self::range(
            ideal,
            RangeInclusive {
                start: ideal - tolerance,
                last: ideal + tolerance,
            },
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[should_panic]
    fn ideal_value_needs_to_be_in_range() {
        ToleratedValues::range(8, RangeInclusive::from(1..=5));
    }

    #[test]
    fn ideal_value_and_tolerance() {
        let a = ToleratedValues::new(8, 2);
        assert_eq!(a.range, RangeInclusive::from(6..=10))
    }
}
