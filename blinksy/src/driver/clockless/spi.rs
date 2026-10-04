use core::marker::PhantomData;

use bitvec::prelude::*;
use bitvec::view::BitView;
use bitvec::view::BitViewSized;
use embedded_hal::spi::SpiBus;
use fugit::HertzU32;
use fugit::NanosDurationU32;

use crate::driver::{ClocklessLed, ClocklessWriter};

pub const fn buffer_size<Led: ClocklessLed, S, Word>(pixel_count: usize, freq_hz: u32) -> usize
where
    S: SpiBus<Word>,
    Word: Copy + 'static,
{
    let clock_period: NanosDurationU32 = HertzU32::Hz(freq_hz).into_duration();
    let clock_period_ns = clock_period.to_nanos();
    // TODO: Use multiply instead of divide
    let t_0h = (Led::T_0H.to_nanos() / clock_period_ns) as usize;
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

pub fn max_error_ns<Led: ClocklessLed>(freq_hz: u32) -> u32 {
    max_error::<Led>(NanosDurationU32::Hz(freq_hz)).to_nanos()
}

fn max_error<Led: ClocklessLed>(clock_period: NanosDurationU32) -> NanosDurationU32 {
    // // Calculate the clock cycles for each required duration
    let t_0h = Led::T_0H / clock_period;
    let t_0l = Led::T_0L / clock_period;
    let t_1h = Led::T_1H / clock_period;
    let t_1l = Led::T_1L / clock_period;

    let error_0h = Led::T_0H - t_0h * clock_period;
    let error_0l = Led::T_0L - t_0l * clock_period;
    let error_1h = Led::T_1H - t_1h * clock_period;
    let error_1l = Led::T_1L - t_1l * clock_period;

    error_0h.max(error_0l).max(error_1h).max(error_1l)
}

// Brute-force an "ideal" clock frequency to run the SPI bus at
// Lower frequencies are better because they mean that we need fewer spi bits to
// encode a single LED bit which means less processing and memory usage. However
// lower frequencies increase the timing errors.
// Each LED has a tolerance for timing variations. We take advantage of this to pick
// the lowest clock frequency that gives us errors within our chosen target tolerance.
pub fn ideal_spi_frequency_hz<Led: ClocklessLed>(target_tolerance_ns: u32) -> u32 {
    // There's going to be some smart ways of doing this but for the time being
    // let's just do the simplest possible thing and explore a whole range of timings
    // and see what works best.
    let target_tolerance = NanosDurationU32::nanos(target_tolerance_ns);
    let duty_cycle = (Led::T_0H + Led::T_0L).max(Led::T_1H + Led::T_1L);
    let mut max_clock_period = NanosDurationU32::nanos(0);
    for clock_period_ns in 1..duty_cycle.to_nanos() {
        let clock_period = NanosDurationU32::nanos(clock_period_ns);
        let error = max_error::<Led>(clock_period);
        if error < target_tolerance && clock_period > max_clock_period {
            max_clock_period = clock_period;
        }
    }
    let freq: HertzU32 = max_clock_period.into_rate();
    freq.to_Hz()
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

struct Pulses {
    zero: PulseCode,
    one: PulseCode,
}

impl Pulses {
    fn new<Led: ClocklessLed>(freq: HertzU32) -> Self {
        let clock_period: NanosDurationU32 = freq.into_duration();

        // // Calculate the clock cycles for each required duration
        let t_0h = Led::T_0H / clock_period;
        let t_0l = Led::T_0L / clock_period;
        let t_1h = Led::T_1H / clock_period;
        let t_1l = Led::T_1L / clock_period;

        #[cfg(feature = "defmt")]
        defmt::info!("Max error: {}", max_error::<Led>(clock_period));

        // TODO: Check that values are within tolerance. Otherwise return an error

        Self {
            zero: PulseCode::new(t_0h as usize, t_0l as usize),
            one: PulseCode::new(t_1h as usize, t_1l as usize),
        }
    }

    fn get(&self, value: bool) -> &PulseCode {
        if value {
            &self.one
        } else {
            &self.zero
        }
    }
}

pub struct SpiWriter<Word, S, const BUFFER_SIZE: usize>
where
    Word: Copy + 'static,
    S: SpiBus<Word>,
{
    spi: S,
    pulses: Pulses,
    word: PhantomData<Word>,
}

impl<Word: Copy + 'static, S: SpiBus<Word>, const BUFFER_SIZE: usize>
    SpiWriter<Word, S, BUFFER_SIZE>
{
    pub fn new<Led: ClocklessLed>(spi: S, freq_hz: u32) -> Self {
        Self {
            spi,
            word: PhantomData,
            pulses: Pulses::new::<Led>(HertzU32::Hz(freq_hz)),
        }
    }

    fn write_impl<Led, const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), S::Error>
    where
        Word: Copy + 'static,
        [Word; BUFFER_SIZE]: BitViewSized,
        Led: ClocklessLed,
        Led::Word: BitView,
    {
        let mut buffer = BitArray::<[Word; BUFFER_SIZE], Msb0>::ZERO;
        let mut dest = buffer.as_mut_bitslice();

        for v in frame {
            for bit in v.view_bits::<Msb0>() {
                let pattern = self.pulses.get(*bit);
                dest[..pattern.len()].clone_from_bitslice(pattern.bits());
                dest = &mut dest[pattern.len()..]
            }
        }
        // For the reset signal we're depending on the rest of the buffer which is full of zeros and
        // should be the correct length
        self.spi.write(&buffer.into_inner())
    }
}

impl<Word, S, Led, const BUFFER_SIZE: usize> ClocklessWriter<Led>
    for SpiWriter<Word, S, BUFFER_SIZE>
where
    Word: Copy + 'static,
    S: SpiBus<Word>,
    [Word; BUFFER_SIZE]: BitViewSized,
    Led: ClocklessLed,
    Led::Word: BitView,
{
    type Error = S::Error;

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        self.write_impl::<Led, FRAME_BUFFER_SIZE>(frame)
    }
}
