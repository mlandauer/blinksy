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
use esp_hal::main;
use esp_hal::spi::master::Config;
use esp_hal::spi::master::Spi;
use esp_hal::{self as hal, delay::Delay};
//use panic_rtt_target as _;
use esp_hal::time::Rate;
use esp_hal::Blocking;
use panic_rtt_target as _;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    rtt_target::rtt_init_defmt!();

    let cpu_clock = hal::clock::CpuClock::max();
    let config = hal::Config::default().with_cpu_clock(cpu_clock);
    let p = hal::init(config);

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

    // let ws2812_driver = {
    //     // Set this to the data pin your LED is connected to
    //     let data_pin = p.GPIO17;
    //     let rmt_clk_freq = hal::time::Rate::from_mhz(80);

    //     let rmt = hal::rmt::Rmt::new(p.RMT, rmt_clk_freq).unwrap();
    //     let rmt_channel = rmt.channel0;

    //     ClocklessDriver::default().with_led::<Ws2812>().with_writer(
    //         ClocklessRmtBuilder::default()
    //             .with_rmt_buffer_size::<{ Layout::PIXEL_COUNT * 3 * 8 + 1 }>()
    //             .with_led::<Ws2812>()
    //             .with_channel(rmt_channel)
    //             .with_pin(data_pin)
    //             .build(),
    //     )
    // };

    const SPI_FREQ: Rate = Rate::from_khz(2500);
    let spi = Spi::new(p.SPI2, Config::default().with_frequency(SPI_FREQ))
        .unwrap()
        .with_mosi(p.GPIO17);
    defmt::info!(
        "spi buffer size: {}",
        blinksy::driver::spi::buffer_size::<Ws2812, Spi<Blocking>, _>(1, SPI_FREQ.as_hz())
    );
    let writer = SpiWriter::<
        _,
        _,
        { blinksy::driver::spi::buffer_size::<Ws2812, Spi<Blocking>, _>(1, SPI_FREQ.as_hz()) },
    >::new(spi, SPI_FREQ.as_hz());
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
