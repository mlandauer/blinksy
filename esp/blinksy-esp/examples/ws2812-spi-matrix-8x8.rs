#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use blinksy::driver::spi::SpiWriter;
use blinksy::{
    driver::ClocklessDriver,
    layout::{Layout2d, Shape2d, Vec2},
    layout2d,
    leds::Ws2812,
    patterns::rainbow::{Rainbow, RainbowParams},
    ControlBuilder,
};
use blinksy_esp::time::elapsed;
use esp_alloc as _;
use esp_hal::spi::master::Config;
use esp_hal::spi::master::Spi;
use esp_hal::{self as hal, delay::Delay};
//use panic_rtt_target as _;
use esp_hal::time::Rate;
use panic_rtt_target as _;
use esp_hal::Blocking;
use embassy_executor::Spawner;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use blinksy::driver::spi::ideal_spi_frequency_hz;
use blinksy::driver::spi::max_error_ns_from_freq_hz;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    rtt_target::rtt_init_defmt!();

    let cpu_clock = hal::clock::CpuClock::max();
    let config = hal::Config::default().with_cpu_clock(cpu_clock);
    let p = hal::init(config);

    let sw_int = SoftwareInterruptControl::new(p.SW_INTERRUPT);
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    layout2d!(
        Layout,
        [Shape2d::Grid {
            start: Vec2::new(1., -1.),
            horizontal_end: Vec2::new(-1., -1.),
            vertical_end: Vec2::new(1., 1.),
            horizontal_pixel_count: 8,
            vertical_pixel_count: 8,
            serpentine: false
        }]
    );

    // let's see what an ideal SPI frequency would be
    const SPI_FREQ_HZ: u32 = ideal_spi_frequency_hz::<Ws2812>(150);
    let error_ns = max_error_ns_from_freq_hz::<Ws2812>(SPI_FREQ_HZ);
    defmt::info!("ideal freq: {} Hz, error: {} ns", SPI_FREQ_HZ, error_ns);

    const SPI_FREQ: Rate = Rate::from_hz(SPI_FREQ_HZ);
    let spi = Spi::new(p.SPI2, Config::default().with_frequency(SPI_FREQ))
        .unwrap()
        .with_mosi(p.GPIO17);
    let writer = SpiWriter::<
        _,
        _,
        {
            blinksy::driver::spi::buffer_size::<Ws2812, Spi<Blocking>, _>(
                Layout::PIXEL_COUNT,
                SPI_FREQ.as_hz(),
            )
        },
    >::new::<Ws2812>(spi, SPI_FREQ.as_hz());
    let driver = ClocklessDriver::default()
        .with_led::<Ws2812>()
        .with_writer(writer);

    let mut control = ControlBuilder::new_2d()
        .with_layout::<Layout, { Layout::PIXEL_COUNT }>()
        .with_pattern::<Rainbow>(RainbowParams {
            ..Default::default()
        })
        .with_driver(driver)
        .with_frame_buffer_size::<{ Ws2812::frame_buffer_size(Layout::PIXEL_COUNT) }>()
        .build();
    control.set_brightness(0.1); // Set initial brightness (0.0 to 1.0)

    let delay = Delay::new();

    loop {
        let elapsed_in_ms = elapsed().as_millis();
        control.tick(elapsed_in_ms).unwrap();
        delay.delay_millis(50);
    }
}
