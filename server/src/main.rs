#![no_std]
#![no_main]

use bsp::board;
use teensy4_bsp as bsp;
use teensy4_panic as _;
use bsp::hal::timer::Blocking;
use usb_device::prelude::*;
use usbd_serial::{SerialPort, USB_CLASS_CDC};

/// Milliseconds to delay before toggling the LED and writing text outputs.
const DELAY_MS: u32 = 1000;

/// Static allocations for use with USB serial
static EP_MEMORY: EndpointMemory<1024> = EndpointMemory::new();
static EP_STATE: EndpointState = EndpointState::max_endpoints();

#[bsp::rt::entry]
fn main() -> ! {
    // These are peripheral instances. Let the board configure these for us.
    // This function can only be called once!
    let instances = board::instances();

    // Driver resources that are configured by the board. For more information,
    // see the `board` documentation.
    let board::Resources {
        // `pins` has objects that represent the physical pins. The object
        // for pin 13 is `p13`.
        pins,
        // This is a hardware timer. We'll use it for blocking delays.
        mut gpt1,
        // These are low-level USB resources. We'll pass these to a function
        // that sets up USB logging.
        usb,
        // This is the GPIO2 port. We need this to configure the LED as a
        // GPIO output.
        mut gpio2,
        lpuart2,
        ..
    } = board::t41(instances);

    // This configures the LED as a GPIO output.
    let led = board::led(&mut gpio2, pins.p13);

    // Configures the GPT1 timer to run at GPT1_FREQUENCY. See the
    // constants below for more information.
    gpt1.disable();
    gpt1.set_divider(GPT1_DIVIDER);
    gpt1.set_clock_source(GPT1_CLOCK_SOURCE);

    // Configures the USB as an CDC-ACM type device - for performing serial communications
    let bus_adapter = BusAdapter::new(
        usb,
        &EP_MEMORY,
        &EP_STATE,
    );
    let bus_allocator = UsbBusAllocator::new(bus_adapter);
    let mut serial = SerialPort::new(&bus_allocator);
    let mut usb_dev = UsbDeviceBuilder::new(&bus_allocator, UsbVidPid(0x5824, 0x27dd))
        .product("Serial Port")
        .device_class(USB_CLASS_CDC)
        .build();

    // Convenience for blocking delays.
    let mut delay = Blocking::<_, GPT1_FREQUENCY>::from_gpt(gpt1);

    loop {
        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];

            match serial.read(&mut buf[..]) {
                Ok(count) => {
                    // count bytes were read to &buf[..count]
                },

                // No data received
                Err(UsbError::WouldBlock) => {}

                // An error occurred
                Err(_err) => {
                    flash_led(&led, &mut delay, 5);
                }
            };

            match serial.write(&[0x3a, 0x29]) {
                Ok(_count) => {
                    // count bytes were written
                },

                // No data could be written (buffers full)
                Err(UsbError::WouldBlock) => {},

                // An error occurred
                Err(_err) => {
                    flash_led(&led, &mut delay, 3);
                }
            };
        }

        delay.block_ms(DELAY_MS);
    }
}

// make the LED flash a lot if we get an error
fn flash_led<const N: u8>(led: &Led, delay: &mut BlockingGpt<N, GPT1_FREQUENCY>, n_fast: u8) {
    const DELAY_FAST: u32 = 50;

    // fast burst
    for _ in 0..n_fast {
        led.toggle();
        delay.block_ms(DELAY_FAST);
        led.toggle();
        delay.block_ms(DELAY_FAST);
    }
}

// We're responsible for configuring our timers.
// This example uses PERCLK_CLK as the GPT1 clock source,
// and it configures a 1 KHz GPT1 frequency by computing a
// GPT1 divider.
use bsp::hal::gpt::ClockSource;
use teensy4_bsp::board::Led;
use teensy4_bsp::hal::timer::BlockingGpt;
use teensy4_bsp::hal::usbd::{BusAdapter, EndpointMemory, EndpointState};
use usb_device::bus::UsbBusAllocator;

/// The intended GPT1 frequency (Hz).
const GPT1_FREQUENCY: u32 = 1_000;
/// Given this clock source...
const GPT1_CLOCK_SOURCE: ClockSource = ClockSource::HighFrequencyReferenceClock;
/// ... the root clock is PERCLK_CLK. To configure a GPT1 frequency,
/// we need a divider of...
const GPT1_DIVIDER: u32 = board::PERCLK_FREQUENCY / GPT1_FREQUENCY;
