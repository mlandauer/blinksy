use crate::driver::ClocklessLed;
use bitvec::{array::BitArray, order::Msb0, slice::BitSlice};
use core::marker::PhantomData;

pub const fn freq_hz_to_duration_ns(freq_hz: u32) -> u32 {
    1_000_000_000 / freq_hz
}

pub const fn duration_ns_to_freq_hz(duration_ns: u32) -> u32 {
    1_000_000_000 / duration_ns
}

pub struct Timing<Led: ClocklessLed> {
    clock_period_ns: u32,
    pub t_0h: u32,
    pub t_0l: u32,
    pub t_1h: u32,
    pub t_1l: u32,
    pub t_reset: u32,
    _led: PhantomData<Led>,
}

impl<Led: ClocklessLed> Timing<Led> {
    pub const fn new(clock_period_ns: u32) -> Self {
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

    pub const fn duty_cycle(&self) -> u32 {
        let t0 = self.t0();
        let t1 = self.t1();
        // We can't yet use max in const function
        if t0 > t1 {
            t0
        } else {
            t1
        }
    }

    pub const fn max_error_ns(&self) -> u32 {
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

pub struct PulseCode {
    // For the moment we're going to assume that we can fit a pulsecode inside one byte
    // TODO: Handle more general case
    buffer: BitArray<[u8; 1], Msb0>,
    len: usize,
}

impl PulseCode {
    pub fn new(high: usize, low: usize) -> Self {
        let mut buffer = BitArray::new([0u8; 1]);
        for mut v in &mut buffer[..high] {
            v.set(true);
        }
        Self {
            buffer,
            len: high + low,
        }
    }

    pub fn bits(&self) -> &BitSlice<u8, Msb0> {
        &self.buffer[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

pub struct Pulses {
    zero: PulseCode,
    one: PulseCode,
}

impl Pulses {
    pub fn new<Led: ClocklessLed>(freq_hz: u32) -> Self {
        let clock_period_ns = freq_hz_to_duration_ns(freq_hz);
        let timing = Timing::<Led>::new(clock_period_ns);
        Self {
            zero: PulseCode::new(timing.t_0h as usize, timing.t_0l as usize),
            one: PulseCode::new(timing.t_1h as usize, timing.t_1l as usize),
        }
    }

    pub fn get(&self, value: bool) -> &PulseCode {
        if value {
            &self.one
        } else {
            &self.zero
        }
    }
}
