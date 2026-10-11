#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BitOrder {
    MostSignificantBit,
    LeastSignificantBit,
}

pub trait Word:
    Copy
    + PartialEq
    + core::ops::BitAnd<Output = Self>
    + core::ops::BitOr<Output = Self>
    + core::ops::Not<Output = Self>
    + core::ops::Shl<usize, Output = Self>
    + core::ops::Shr<usize, Output = Self>
{
    const BITS: u32;
    const ZERO: Self;
    const ONE: Self;
}

macro_rules! impl_word {
    ($t:ty) => {
        impl Word for $t {
            const BITS: u32 = <$t>::BITS;
            const ZERO: Self = 0 as $t;
            const ONE: Self = 1 as $t;
        }
    };
}

impl_word!(u8);
impl_word!(u16);
impl_word!(u32);
impl_word!(u64);
impl_word!(u128);

/// MSB-first bit iterator
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BitsMsb<W: Word> {
    value: W,
    mask: W,
    remaining: u32,
}

impl<W: Word> BitsMsb<W> {
    #[inline]
    pub fn new(word: W) -> Self {
        // Safe because T::BITS >= 8 for all supported primitives.
        let top = (W::BITS - 1) as usize;
        Self {
            value: word,
            mask: W::ONE << top,
            remaining: W::BITS,
        }
    }
}

impl<W: Word> Iterator for BitsMsb<W> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.remaining == 0 {
            return None;
        }
        let bit = (self.value & self.mask) != W::ZERO;
        self.mask = self.mask >> 1;
        self.remaining -= 1;
        Some(bit)
    }
}

#[inline]
pub fn word_to_bits_msb<W: Word>(word: W) -> BitsMsb<W> {
    BitsMsb::new(word)
}

/// Copies the first `len` bits of `src` into `dst`, starting at bit `dst_start`
pub fn copy_bits_msb<W: Word>(src: &[W], len: usize, dst: &mut [W], dst_start: usize) {
    let word_bits = W::BITS as usize;
    let dst_first_index = dst_start / word_bits;
    let dst_shift = dst_start % word_bits;

    for (i, &src_word) in src[..len.div_ceil(word_bits)].iter().enumerate() {
        let dst_index = dst_first_index + i;
        // Number of bits to copy in this word
        let count = (len - i * word_bits).min(word_bits);
        // Selects the top `count` bits, which are the ones being copied
        let mask = !W::ZERO << (word_bits - count);
        let src_masked_word = src_word & mask;

        // The bits that fit into the current destination word
        dst[dst_index] = (dst[dst_index] & !(mask >> dst_shift)) | (src_masked_word >> dst_shift);
        // The bits that overflow into the next destination word
        if dst_shift + count > word_bits {
            let back = word_bits - dst_shift;
            dst[dst_index + 1] = (dst[dst_index + 1] & !(mask << back)) | (src_masked_word << back);
        }
    }
}

/// Appends bits to a slice of words, filling each word from its most significant bit
pub struct BitWriterMsb<'a, W: Word> {
    words: &'a mut [W],
    position: usize,
}

impl<'a, W: Word> BitWriterMsb<'a, W> {
    pub fn new(words: &'a mut [W]) -> Self {
        Self { words, position: 0 }
    }

    /// Appends the first `len` bits of `src`
    pub fn write_bits(&mut self, src: &[W], len: usize) {
        copy_bits_msb(src, len, self.words, self.position);
        self.position += len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heapless::Vec;

    #[test]
    fn test_u8_msb() {
        let bits: Vec<bool, 8> = word_to_bits_msb(0b1010_0001_u8).collect();

        assert_eq!(
            bits,
            Vec::<bool, 8>::from_array([true, false, true, false, false, false, false, true])
        );
    }

    #[test]
    fn test_u16_msb() {
        let bits: Vec<bool, 16> = word_to_bits_msb(0x0408_u16).collect();

        assert_eq!(
            bits,
            Vec::<bool, 16>::from_array([
                false, false, false, false, false, true, false, false, // 0x04
                false, false, false, false, true, false, false, false, // 0x08
            ])
        );
    }

    #[test]
    fn test_copy_bits_msb_simple() {
        let mut dst: [u8; 2] = [0b1111_111, 0b1111_1111];
        copy_bits_msb(&[0b0000_0000, 0b0000_0000], 11, &mut dst, 0);

        assert_eq!(dst, [0b0000_0000, 0b0001_1111]);
    }

    #[test]
    fn test_copy_bits_msb_across_word_boundary() {
        let mut dst: [u8; 3] = [0b1111_1111, 0b1111_1111, 0b1111_1111];
        copy_bits_msb(&[0b0000_0000, 0b0000_0000], 11, &mut dst, 5);

        assert_eq!(dst, [0b1111_1000, 0b0000_0000, 0b1111_1111]);
    }
}
