#![no_std]
#![no_main]

use arduino_hal::prelude::*;
use aucpace::AuCPaceClient;
use panic_halt as _;
use rand_chacha::ChaChaRng;
use rand_chacha::rand_core::{SeedableRng, RngCore};

#[no_mangle]
extern "C" fn abort() {
    loop {}
}

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);
    let mut serial = arduino_hal::default_serial!(dp, pins, 57600);

    let mut rng = ChaChaRng::seed_from_u64(0xCAFEBABE);
    let _client = AuCPaceClient::new(rng);

    ufmt::uwriteln!(&mut serial, "Hello from Arduino!\r").void_unwrap();

    loop {
        // Read a byte from the serial connection
        let b = nb::block!(serial.read()).void_unwrap();

        // Answer
        ufmt::uwriteln!(&mut serial, "Got {}! - have a random number for free: {}\r", b, rng.next_u64()).void_unwrap();
    }
}
