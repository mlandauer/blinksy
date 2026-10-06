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

struct Timing<Led: ClocklessLed> {
    clock_period_ns: u32,
    t_0h: u32,
    t_0l: u32,
    t_1h: u32,
    t_1l: u32,
    t_reset: u32,
    _led: PhantomData<Led>,
}

impl<Led: ClocklessLed> Timing<Led> {
    const fn new(clock_period_ns: u32) -> Self {
        Self {
            clock_period_ns,
            t_0h: Led::T_0H.to_nanos() / clock_period_ns,
            t_0l: Led::T_0L.to_nanos() / clock_period_ns,
            t_1h: Led::T_1H.to_nanos() / clock_period_ns,
            t_1l: Led::T_1L.to_nanos() / clock_period_ns,
            t_reset: Led::T_RESET.to_nanos() / clock_period_ns,
            _led: PhantomData,
        }
    }

    const fn t0(&self) -> u32 {
        self.t_0h + self.t_0l
    }

    const fn t1(&self) -> u32 {
        self.t_1h + self.t_1l
    }

    const fn duty_cycle(&self) -> u32 {
        let t0 = self.t0();
        let t1 = self.t1();
        // We can't yet use max in const function
        if t0 > t1 {
            t0
        } else {
            t1
        }
    }

    const fn max_error_ns(&self) -> u32 {
        let error_0h_ns = Led::T_0H.to_nanos() - self.t_0h * self.clock_period_ns;
        let error_0l_ns = Led::T_0L.to_nanos() - self.t_0l * self.clock_period_ns;
        let error_1h_ns = Led::T_1H.to_nanos() - self.t_1h * self.clock_period_ns;
        let error_1l_ns = Led::T_1L.to_nanos() - self.t_1l * self.clock_period_ns;

        let mut max_error_ns = 0;
        if error_0h_ns > max_error_ns {
            max_error_ns = error_0h_ns
        };
        if error_0l_ns > max_error_ns {
            max_error_ns = error_0l_ns
        };
        if error_1h_ns > max_error_ns {
            max_error_ns = error_1h_ns
        };
        if error_1l_ns > max_error_ns {
            max_error_ns = error_1l_ns
        };
        max_error_ns
    }
}

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

pub const fn duty_cycle_bits_from_frequency_hz<Led: ClocklessLed>(freq_hz: u32) -> u32 {
    let clock_period_ns = freq_hz_to_duration_ns(freq_hz);
    let timing = Timing::<Led>::new(clock_period_ns);
    timing.duty_cycle()
}

const fn max_error_ns_from_freq_hz<Led: ClocklessLed>(freq_hz: u32) -> u32 {
    let clock_period_ns = freq_hz_to_duration_ns(freq_hz);
    max_error_ns_from_clock_period_ns::<Led>(clock_period_ns)
}

const fn max_error_ns_from_clock_period_ns<Led: ClocklessLed>(clock_period_ns: u32) -> u32 {
    let timing = Timing::<Led>::new(clock_period_ns);
    timing.max_error_ns()
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
        let error_ns = max_error_ns_from_clock_period_ns::<Led>(clock_period_ns);
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
    fn new<Led: ClocklessLed>(freq_hz: u32) -> Self {
        let clock_period_ns = freq_hz_to_duration_ns(freq_hz);
        let timing = Timing::<Led>::new(clock_period_ns);
        Self {
            zero: PulseCode::new(timing.t_0h as usize, timing.t_0l as usize),
            one: PulseCode::new(timing.t_1h as usize, timing.t_1l as usize),
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

const fn freq_hz_to_duration_ns(freq_hz: u32) -> u32 {
    1_000_000_000 / freq_hz
}

const fn duration_ns_to_freq_hz(duration_ns: u32) -> u32 {
    1_000_000_000 / duration_ns
}

pub struct ClocklessSpi<Word, S, const BUFFER_SIZE: usize>
where
    Word: Copy + 'static,
    S: SpiBus<Word>,
{
    freq_hz: u32,
    spi: S,
    pulses: Pulses,
    word: PhantomData<Word>,
}

#[cfg(feature = "async")]
pub struct ClocklessSpiAsync<Word, S, const BUFFER_SIZE: usize>
where
    Word: Copy + 'static,
    S: SpiBusAsync<Word>,
{
    freq_hz: u32,
    spi: S,
    pulses: Pulses,
    word: PhantomData<Word>,
}

impl<Word: Copy + 'static, S: SpiBus<Word>, const BUFFER_SIZE: usize>
    ClocklessSpi<Word, S, BUFFER_SIZE>
{
    pub fn new<Led: ClocklessLed>(spi: S, freq_hz: u32) -> Self {
        Self {
            freq_hz,
            spi,
            word: PhantomData,
            pulses: Pulses::new::<Led>(freq_hz),
        }
    }

    // TODO: Don't want to have to use Led here
    pub fn max_error_ns<Led: ClocklessLed>(&self) -> u32 {
        max_error_ns_from_freq_hz::<Led>(self.freq_hz)
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

// TODO: Extract common bits
#[cfg(feature = "async")]
impl<Word: Copy + 'static, S: SpiBusAsync<Word>, const BUFFER_SIZE: usize>
    ClocklessSpiAsync<Word, S, BUFFER_SIZE>
{
    pub fn new<Led: ClocklessLed>(spi: S, freq_hz: u32) -> Self {
        Self {
            freq_hz,
            spi,
            word: PhantomData,
            pulses: Pulses::new::<Led>(freq_hz),
        }
    }

    // TODO: Don't want to have to use Led here
    pub fn max_error_ns<Led: ClocklessLed>(&self) -> u32 {
        max_error_ns_from_freq_hz::<Led>(self.freq_hz)
    }

    async fn write_impl<Led, const FRAME_BUFFER_SIZE: usize>(
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
        self.spi.write(&buffer.into_inner()).await
    }
}

impl<Word, S, Led, const BUFFER_SIZE: usize> ClocklessWriter<Led>
    for ClocklessSpi<Word, S, BUFFER_SIZE>
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

#[cfg(feature = "async")]
impl<Word, S, Led, const BUFFER_SIZE: usize> ClocklessWriterAsync<Led>
    for ClocklessSpiAsync<Word, S, BUFFER_SIZE>
where
    Word: Copy + 'static,
    S: SpiBusAsync<Word>,
    [Word; BUFFER_SIZE]: BitViewSized,
    Led: ClocklessLed,
    Led::Word: BitView,
{
    type Error = S::Error;

    async fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        self.write_impl::<Led, FRAME_BUFFER_SIZE>(frame).await
    }
}
