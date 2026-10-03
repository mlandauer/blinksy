use bitvec::prelude::*;
use embedded_hal::spi::SpiBus;
use fugit::MegahertzU32;
use fugit::NanosDurationU32;

use crate::driver::{ClocklessLed, ClocklessWriter};

pub struct SpiWriter<Spi: SpiBus> {
    pub spi: Spi,
}

impl<S: SpiBus, Led: ClocklessLed> ClocklessWriter<Led> for SpiWriter<S> {
    type Error = S::Error;

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        _frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
    ) -> Result<(), Self::Error> {
        // To start with let's assume the spi frequency is set to 3 Mhz. We need to calculate
        // the bit pattern to send to spi to make everything work
        let freq = MegahertzU32::MHz(3);
        let clock_period: NanosDurationU32 = freq.into_duration();
        // Calculate the clock cycles for each required duration
        let t_0h = Led::T_0H / clock_period;
        let t_0l = Led::T_0L / clock_period;
        let t_1h = Led::T_1H / clock_period;
        let t_1l = Led::T_1L / clock_period;
        let t_reset = Led::T_RESET / clock_period;

        // Overall a one should take exactly the same amount to transmit as a zero
        assert_eq!(t_0h + t_0l, t_1h + t_1l);

        // Right now we're making this assumption
        assert_eq!(t_0h + t_0l, 3);
        assert_eq!(t_reset, 150);

        // For the time being we're going to assume that the spi buffer is going to be big
        // enough. In reality, we will have to do some things to make sure that's true
        // const SPI_BUFFER_SIZE_BITS: usize = 64 * 3 * 8 * 3 + 140;
        // const SPI_BUFFER_SIZE: usize = SPI_BUFFER_SIZE_BITS.div_ceil(8);

        #[cfg(feature = "defmt")]
        defmt::info!(
            "0: {} {}, 1: {} {}, reset: {}",
            t_0h,
            t_0l,
            t_1h,
            t_1l,
            t_reset
        );

        // Just to get started we'll just send 24 bits of one (2 high, followed by 1 low) to the LED
        // type SpiBuffer = BitArr!(for 24 * 3, in u8);
        let mut buffer = bitarr![u8, Msb0; 0; 24 * 3 + 150];
        let one = bitarr![u8, Msb0; 1, 1, 0];
        for v in buffer[0..24 * 3].chunks_exact_mut(3) {
            v.clone_from_bitslice(&one.as_bitslice()[0..3]);
        }
        #[cfg(feature = "defmt")]
        defmt::info!("{}", buffer.as_raw_slice());
        self.spi.write(buffer.as_raw_slice())
    }
}
