use core::marker::PhantomData;

use bitvec::prelude::*;
use bitvec::view::BitView;
use bitvec::view::BitViewSized;
use embedded_hal::spi::SpiBus;
#[cfg(feature = "async")]
use embedded_hal_async::spi::SpiBus as SpiBusAsync;

#[cfg(feature = "async")]
use crate::driver::ClocklessWriterAsync;
use crate::driver::{ClocklessLed, ClocklessWriter};

mod encoding;
use encoding::{duration_ns_to_freq_hz, freq_hz_to_duration_ns, Pulses, Timing};

pub const fn clockless_spi_buffer_size<Led: ClocklessLed, S, Word>(
    pixel_count: usize,
    freq_hz: u32,
) -> usize
where
    S: SpiBus<Word>,
    Word: Copy + 'static,
{
    let clock_period_ns = freq_hz_to_duration_ns(freq_hz);
    let timing = Timing::<Led>::new(clock_period_ns);

    // TODO: Check that resulting timings are within spec for the LED and error if not
    let spi_word_bits = size_of::<Word>() * 8;
    let total_bits = spi_word_bits
        * pixel_count
        * Led::LED_CHANNELS.channel_count()
        * timing.duty_cycle() as usize
        + timing.t_reset as usize;
    total_bits.div_ceil(spi_word_bits)
}

// Brute-force an "ideal" clock frequency to run the SPI bus at
// Lower frequencies are better because they mean that we need fewer spi bits to
// encode a single LED bit which means less processing and memory usage. However
// lower frequencies increase the timing errors.
// Each LED has a tolerance for timing variations. We take advantage of this to pick
// the lowest clock frequency that gives us errors within our chosen target tolerance.
pub const fn clockless_spi_ideal_frequency_hz<Led: ClocklessLed>(target_tolerance_ns: u32) -> u32 {
    // There's going to be some smart ways of doing this but for the time being
    // let's just do the simplest possible thing and explore a whole range of timings
    // and see what works best.
    let t0_ns = Led::T_0H.to_nanos() + Led::T_0L.to_nanos();
    let t1_ns = Led::T_1H.to_nanos() + Led::T_1L.to_nanos();
    let duty_cycle_ns = if t0_ns > t1_ns { t0_ns } else { t1_ns };
    let mut max_clock_period_ns = 0;
    let mut clock_period_ns = 1;
    loop {
        let error_ns = Timing::<Led>::new(clock_period_ns).max_error_ns();
        if error_ns < target_tolerance_ns && clock_period_ns > max_clock_period_ns {
            max_clock_period_ns = clock_period_ns;
        }
        clock_period_ns += 1;
        if clock_period_ns >= duty_cycle_ns {
            break;
        }
    }
    duration_ns_to_freq_hz(max_clock_period_ns)
}

pub struct ClocklessSpi<const BUFFER_SIZE: usize, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    Spi: SpiBus<SpiWord>,
    SpiWord: Copy + 'static,
{
    spi: Spi,
    timing: Timing<Led>,
    pulses: Pulses,
    _spi_word: PhantomData<SpiWord>,
}

#[cfg(feature = "async")]
pub struct ClocklessSpiAsync<const BUFFER_SIZE: usize, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    Spi: SpiBusAsync<SpiWord>,
    SpiWord: Copy + 'static,
{
    spi: Spi,
    timing: Timing<Led>,
    pulses: Pulses,
    _spi_word: PhantomData<SpiWord>,
}

impl<const BUFFER_SIZE: usize, Led, Spi, SpiWord> ClocklessSpi<BUFFER_SIZE, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    Spi: SpiBus<SpiWord>,
    SpiWord: Copy + 'static,
{
    pub fn new(spi: Spi, freq_hz: u32) -> Self {
        let timing = Timing::new(freq_hz_to_duration_ns(freq_hz));
        Self {
            spi,
            pulses: Pulses::new(&timing),
            timing,
            _spi_word: PhantomData,
        }
    }

    pub fn max_error_ns(&self) -> u32 {
        self.timing.max_error_ns()
    }

    pub fn duty_cycle_bits(&self) -> u32 {
        self.timing.duty_cycle()
    }

    fn write_impl<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Spi::Error>
    where
        SpiWord: Copy + 'static,
        [SpiWord; BUFFER_SIZE]: BitViewSized,
        Led: ClocklessLed,
        Led::Word: BitView,
    {
        let mut buffer = BitArray::<[SpiWord; BUFFER_SIZE], Msb0>::ZERO;
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

// TODO: Extract common bits
#[cfg(feature = "async")]
impl<const BUFFER_SIZE: usize, Led, Spi, SpiWord> ClocklessSpiAsync<BUFFER_SIZE, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    SpiWord: Copy + 'static,
    Spi: SpiBusAsync<SpiWord>,
{
    pub fn new(spi: Spi, freq_hz: u32) -> Self {
        let timing = Timing::new(freq_hz_to_duration_ns(freq_hz));
        Self {
            spi,
            pulses: Pulses::new::<Led>(&timing),
            timing,
            _spi_word: PhantomData,
        }
    }

    pub fn max_error_ns(&self) -> u32 {
        self.timing.max_error_ns()
    }

    pub fn duty_cycle_bits(&self) -> u32 {
        self.timing.duty_cycle()
    }

    async fn write_impl<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Spi::Error>
    where
        SpiWord: Copy + 'static,
        [SpiWord; BUFFER_SIZE]: BitViewSized,
        Led::Word: BitView,
    {
        let mut buffer = BitArray::<[SpiWord; BUFFER_SIZE], Msb0>::ZERO;
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
        self.spi.write(&buffer.into_inner()).await
    }
}

impl<const BUFFER_SIZE: usize, Led, Spi, SpiWord> ClocklessWriter<Led>
    for ClocklessSpi<BUFFER_SIZE, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    Led::Word: BitView,
    Spi: SpiBus<SpiWord>,
    SpiWord: Copy + 'static,
    [SpiWord; BUFFER_SIZE]: BitViewSized,
{
    type Error = Spi::Error;

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        self.write_impl::<FRAME_BUFFER_SIZE>(frame)
    }
}

#[cfg(feature = "async")]
impl<const BUFFER_SIZE: usize, Led, Spi, SpiWord> ClocklessWriterAsync<Led>
    for ClocklessSpiAsync<BUFFER_SIZE, Led, Spi, SpiWord>
where
    Led: ClocklessLed,
    Led::Word: BitView,
    Spi: SpiBusAsync<SpiWord>,
    SpiWord: Copy + 'static,
    [SpiWord; BUFFER_SIZE]: BitViewSized,
{
    type Error = Spi::Error;

    async fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        self.write_impl::<FRAME_BUFFER_SIZE>(frame).await
    }
}
