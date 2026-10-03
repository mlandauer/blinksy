use core::marker::PhantomData;

use bitvec::prelude::*;
use bitvec::view::BitViewSized;
use embedded_hal::spi::SpiBus;
use fugit::HertzU32;
use fugit::NanosDurationU32;

use crate::driver::{ClocklessLed, ClocklessWriter};

const fn t_0h<Led: ClocklessLed>(clock_period_ns: u32) -> u32 {
    Led::T_0H.to_nanos() / clock_period_ns
}

pub const fn buffer_size<Led: ClocklessLed, S, Word>(pixel_count: usize, freq_hz: u32) -> usize
where
    S: SpiBus<Word>,
    Word: Copy + 'static,
{
    let clock_period: NanosDurationU32 = HertzU32::Hz(freq_hz).into_duration();
    let clock_period_ns = clock_period.to_nanos();
    // TODO: Use multiply instead of divide
    let t_0h = t_0h::<Led>(clock_period_ns) as usize;
    let t_0l = (Led::T_0L.to_nanos() / clock_period_ns) as usize;
    let t_1h = (Led::T_1H.to_nanos() / clock_period_ns) as usize;
    let t_1l = (Led::T_1L.to_nanos() / clock_period_ns) as usize;
    let t_reset = (Led::T_RESET.to_nanos() / clock_period_ns) as usize;

    // TODO: Check that resulting timings are within spec for the LED and error if not
    // The maximum length a bit could be
    let t0 = t_0h + t_0l;
    let t1 = t_1h + t_1l;
    // We can't yet use max in const function
    let t_max = if t0 > t1 { t0 } else { t1 };
    // let t_max = t0.max(t1);
    let spi_word_bits = size_of::<Word>() * 8;
    let total_bits =
        spi_word_bits * pixel_count * Led::LED_CHANNELS.channel_count() * t_max + t_reset;
    total_bits.div_ceil(spi_word_bits)
}

struct PulseCode {
    // For the moment we're going to assume that we can fit a pulsecode inside one byte
    // TODO: Handle more general case
    buffer: BitArray<[u8; 1], Msb0>,
    len: usize,
}

impl PulseCode {
    fn new(high: usize, low: usize) -> Self {
        let mut buffer = BitArray::new([0u8; 1]);
        for mut v in &mut buffer[..high] {
            v.set(true);
        }
        Self {
            buffer,
            len: high + low,
        }
    }

    fn bits(&self) -> &BitSlice<u8, Msb0> {
        &self.buffer[..self.len]
    }

    fn len(&self) -> usize {
        self.len
    }
}

pub struct SpiWriter<Word, S, const BUFFER_SIZE: usize>
where
    Word: Copy + 'static,
    S: SpiBus<Word>,
{
    spi: S,
    freq: HertzU32,
    word: PhantomData<Word>,
}

impl<Word: Copy + 'static, S: SpiBus<Word>, const BUFFER_SIZE: usize>
    SpiWriter<Word, S, BUFFER_SIZE>
{
    pub fn new(spi: S, freq_hz: u32) -> Self {
        Self {
            spi,
            freq: HertzU32::Hz(freq_hz),
            word: PhantomData,
        }
    }
}

impl<Word, S, Led, const BUFFER_SIZE: usize> ClocklessWriter<Led>
    for SpiWriter<Word, S, BUFFER_SIZE>
where
    Word: Copy + 'static,
    S: SpiBus<Word>,
    [Word; BUFFER_SIZE]: BitViewSized,
    Led: ClocklessLed,
{
    type Error = S::Error;

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        _frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        let clock_period: NanosDurationU32 = self.freq.into_duration();

        // // Calculate the clock cycles for each required duration
        let t_0h = (Led::T_0H / clock_period) as usize;
        let t_0l = (Led::T_0L / clock_period) as usize;
        let t_1h = (Led::T_1H / clock_period) as usize;
        let t_1l = (Led::T_1L / clock_period) as usize;

        // TODO: Check that values are within tolerance. Otherwise return an error

        let mut buffer = BitArray::<[Word; BUFFER_SIZE], Msb0>::ZERO;
        let zero = PulseCode::new(t_0h, t_0l);
        let one = PulseCode::new(t_1h, t_1l);

        let mut dest = buffer.as_mut_bitslice();
        for _ in 0..8 {
            dest[..one.len()].clone_from_bitslice(one.bits());
            dest = &mut dest[one.len()..]
        }
        for _ in 0..16 {
            dest[..zero.len()].clone_from_bitslice(zero.bits());
            dest = &mut dest[zero.len()..]
        }
        self.spi.write(&buffer.into_inner())
    }
}
