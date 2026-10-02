use embedded_hal::spi::SpiBus;
use fugit::MegahertzU32;
use fugit::NanosDurationU32;

use crate::driver::{ClocklessLed, ClocklessWriter};

pub struct SpiWriter<Spi: SpiBus> {
    pub spi: Spi,
}

impl<S: SpiBus, Led: ClocklessLed> ClocklessWriter<Led> for SpiWriter<S> {
    type Error = ();

    fn write<const FRAME_BUFFER_SIZE: usize>(
        &mut self,
        frame: heapless::Vec<Led::Word, FRAME_BUFFER_SIZE>,
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

        #[cfg(feature = "defmt")]
        defmt::info!(
            "0: {} {}, 1: {} {}, reset: {}",
            t_0h,
            t_0l,
            t_1h,
            t_1l,
            t_reset
        );
        todo!()
    }
}
