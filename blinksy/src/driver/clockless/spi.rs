use bitvec::prelude::*;
use embedded_hal::spi::SpiBus;
use fugit::HertzU32;
use fugit::NanosDurationU32;

use crate::driver::{ClocklessLed, ClocklessWriter};

pub struct SpiWriter<S: SpiBus, const BUFFER_SIZE: usize> {
    spi: S,
    freq: HertzU32,
}

const fn t_0h<Led: ClocklessLed>(clock_period_ns: u32) -> u32 {
    Led::T_0H.to_nanos() / clock_period_ns
}

pub const fn buffer_size<Led: ClocklessLed>(freq_hz: u32) -> usize {
    let clock_period: NanosDurationU32 = HertzU32::Hz(freq_hz).into_duration();
    let clock_period_ns = clock_period.to_nanos();
    // TODO: Use multiply instead of divide
    let t_0h = t_0h::<Led>(clock_period_ns);
    let t_0l = Led::T_0L.to_nanos() / clock_period_ns;
    let t_1h = Led::T_1H.to_nanos() / clock_period_ns;
    let t_1l = Led::T_1L.to_nanos() / clock_period_ns;
    let t_reset = Led::T_RESET.to_nanos() / clock_period_ns;

    // TODO: Check that resulting timings are within spec for the LED and error if not
    // The maximum length a bit could be
    let t0 = t_0h + t_0l;
    let t1 = t_1h + t_1l;
    // We can't yet use max in const function
    let t_max = if t0 > t1 { t0 } else { t1 };
    // let t_max = t0.max(t1);

    let total_bits = 24 * t_max + t_reset;
    total_bits.div_ceil(8) as usize
}

impl<S: SpiBus, const BUFFER_SIZE: usize> SpiWriter<S, BUFFER_SIZE> {
    pub fn new(spi: S, freq_hz: u32) -> Self {
        Self {
            spi,
            freq: HertzU32::Hz(freq_hz),
        }
    }

    fn test_write<Led: ClocklessLed>(&mut self) -> Result<(), S::Error> {
        let clock_period: NanosDurationU32 = self.freq.into_duration();

        // // Calculate the clock cycles for each required duration
        //let t_0h = Led::T_0H / clock_period;
        //let t_0l = Led::T_0L / clock_period;
        let t_1h = (Led::T_1H / clock_period) as usize;
        let t_1l = (Led::T_1L / clock_period) as usize;
        //let t_reset = Led::T_RESET / clock_period;
        let t1 = t_1h + t_1l;

        // TODO: Check timings are within the LED spec

        // // Right now we're making this assumption
        // assert_eq!(t_0h + t_0l, 3);
        // assert_eq!(t_reset, 125);

        // // For the time being we're going to assume that the spi buffer is going to be big
        // // enough. In reality, we will have to do some things to make sure that's true
        // // const SPI_BUFFER_SIZE_BITS: usize = 64 * 3 * 8 * 3 + 140;
        // // const SPI_BUFFER_SIZE: usize = SPI_BUFFER_SIZE_BITS.div_ceil(8);

        // #[cfg(feature = "defmt")]
        // defmt::info!(
        //     "0: {} {}, 1: {} {}, reset: {}",
        //     t_0h,
        //     t_0l,
        //     t_1h,
        //     t_1l,
        //     t_reset
        // );

        // // TODO: Check that values are within tolerance. Otherwise return an error

        // defmt::info!("one period high: {} ns", (t_1h * clock_period).to_nanos());
        // defmt::info!("one period low: {} ns", (t_1l * clock_period).to_nanos());

        // const BUFFER_SIZE = 25;
        // Just to get started we'll just send 24 bits of one (2 high, followed by 1 low) to the LED
        // type SpiBuffer = BitArr!(for 24 * 3, in u8);
        let mut buffer = BitArray::<[u8; BUFFER_SIZE], Msb0>::new([0u8; BUFFER_SIZE]);
        // For the moment we're going to assume that we can fit a single bit inside one byte
        let mut one = BitArray::<[u8; 1], Msb0>::new([0u8; 1]);
        for mut v in &mut one[..t_1h] {
            v.set(true);
        }
        let one_slice = &one.as_bitslice()[..t1];
        let mut dest = buffer.as_mut_bitslice();
        for _ in 0..24 {
            dest[..one_slice.len()].clone_from_bitslice(one_slice);
            dest = &mut dest[one_slice.len()..]
        }
        #[cfg(feature = "defmt")]
        defmt::info!("{}", buffer.as_raw_slice());
        self.spi.write(buffer.as_raw_slice())
    }
}

impl<S: SpiBus, Led: ClocklessLed, const BUFFER_SIZE: usize> ClocklessWriter<Led>
    for SpiWriter<S, BUFFER_SIZE>
{
    type Error = S::Error;

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        _frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        self.test_write::<Led>()
    }
}
