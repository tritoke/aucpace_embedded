#![no_std]
#![no_main]

use bsp::board;
use teensy4_bsp as bsp;
use teensy4_panic as _;
use bsp::hal::timer::Blocking;
use usb_device::prelude::*;
use usbd_serial::{SerialPort, USB_CLASS_CDC};
use core::fmt::Write as _;

/// CHANGE ME to vary the baud rate.
const UART_BAUD: u32 = 115200;
/// Milliseconds to delay before toggling the LED and writing text outputs.
const DELAY_MS: u32 = 5;

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
    bus_adapter.gpt_mut(Instance::Gpt0, |gpt| {
        gpt.stop(); // Stop the timer, just in case it's already running...
        gpt.clear_elapsed(); // Clear any outstanding elapsed flags
        gpt.set_interrupt_enabled(false);
        gpt.set_load(4000);
        gpt.set_mode(Mode::Repeat); // Repeat the timer after it elapses
        gpt.reset(); // Load the value into the counter
        gpt.run(); // start the timer running
    });
    bus_adapter.configure();

    // Not sure which endpoints the CDC ACM class will pick,
    // so enable the setting for all non-zero endpoints.
    for idx in 1..8 {
        for dir in &[usb_device::UsbDirection::In, usb_device::UsbDirection::Out] {
            let ep_addr = usb_device::endpoint::EndpointAddress::from_parts(idx, *dir);
            // CDC class requires that we send the ZLP.
            // Let the hardware do that for us.
            bus_adapter.enable_zlt(ep_addr);
        }
    }

    let bus_allocator = UsbBusAllocator::new(bus_adapter);
    let mut serial = SerialPort::new(&bus_allocator);
    // Arduino Serial 16c0:0483
    // imxrt-log 5824:27dd
    let mut usb_dev = UsbDeviceBuilder::new(&bus_allocator, UsbVidPid(0x5824, 0x27dd))
        .manufacturer("ACME Ltd")
        .product("Serial Port")
        .serial_number("69420")
        .device_class(USB_CLASS_CDC)
        .build();

    // Create the UART driver using pins 14 and 15.
    // Cast it to a embedded_hal trait object so we can
    // use it with the write! macro.
    let mut lpuart2: board::Lpuart2 = board::lpuart(lpuart2, pins.p14, pins.p15, UART_BAUD);
    let lpuart2: &mut dyn embedded_hal::serial::Write<u8, Error = _> = &mut lpuart2;

    // Convenience for blocking delays.
    let mut delay = Blocking::<_, GPT1_FREQUENCY>::from_gpt(gpt1);
    let mut counter = 0u32;

    loop {
        if counter % (1000 / DELAY_MS) == 0 {
            led.toggle();
        }

        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];

            match serial.read(&mut buf[..]) {
                Ok(count) => {
                    // count bytes were read to &buf[..count]
                    write!(lpuart2, "Read {:?}\r\n", &buf[..count]).ok();
                },

                // No data received
                Err(UsbError::WouldBlock) => {
                    write!(lpuart2, "Read would block\r\n").ok();
                }

                // An error occurred
                Err(err) => {
                    write!(lpuart2, "Read error - err={err:?}\r\n").ok();
                }
            };

            match serial.write(&[0x3a, 0x29]) {
                Ok(count) => {
                    // count bytes were written
                    write!(lpuart2, "Wrote {count} bytes\r\n").ok();
                },

                // No data could be written (buffers full)
                Err(UsbError::WouldBlock) => {
                    write!(lpuart2, "Write failed - would block\r\n").ok();
                },

                // An error occurred
                Err(err) => {
                    write!(lpuart2, "Write error - err={err:?}\r\n").ok();
                }
            };
        }

        if counter % (1000 / DELAY_MS) == 0 {
            write!(lpuart2, "Nice cock - counter = {counter}\r\n").ok();
        }
        delay.block_ms(DELAY_MS);
        counter = counter.wrapping_add(1);
    }
}

// We're responsible for configuring our timers.
// This example uses PERCLK_CLK as the GPT1 clock source,
// and it configures a 1 KHz GPT1 frequency by computing a
// GPT1 divider.
use bsp::hal::gpt::ClockSource;
use teensy4_bsp::hal::usbd::{BusAdapter, EndpointMemory, EndpointState};
use teensy4_bsp::hal::usbd::gpt::{Instance, Mode};
use usb_device::bus::UsbBusAllocator;

/// The intended GPT1 frequency (Hz).
const GPT1_FREQUENCY: u32 = 1_000;
/// Given this clock source...
const GPT1_CLOCK_SOURCE: ClockSource = ClockSource::HighFrequencyReferenceClock;
/// ... the root clock is PERCLK_CLK. To configure a GPT1 frequency,
/// we need a divider of...
const GPT1_DIVIDER: u32 = board::PERCLK_FREQUENCY / GPT1_FREQUENCY;
