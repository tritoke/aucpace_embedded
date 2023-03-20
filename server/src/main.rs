#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use aucpace::{AuCPaceServer, ClientMessage};
use core::any::Any;
use core::fmt::Write as _;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::interrupt;
use embassy_stm32::usart::{Config, Uart};
use embassy_time::Instant;
use heapless::String;
use rand_chacha::ChaChaRng;
use rand_core::{RngCore, SeedableRng};
use {defmt_rtt as _, panic_probe as _};

const K1: usize = 16;

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());
    info!("Initialised peripherals.");

    // configure USART2 which goes over the USB port on this board
    let config = Config::default();
    let irq = interrupt::take!(USART2);
    let mut usart = Uart::new(p.USART2, p.PA3, p.PA2, irq, p.DMA1_CH6, p.DMA1_CH5, config);
    info!("Configured USART2.");

    // configure the RNG, kind of insecure but this is just a demo and I don't have real entropy
    let now = Instant::now().as_micros();
    let mut rng = ChaChaRng::seed_from_u64(now);
    info!("Seeded RNG - seed = {}", now);

    // create our AuCPace server
    let _server: AuCPaceServer<sha2::Sha512, _, K1> = AuCPaceServer::new(rng);
    // let mut curr_state: Option<dyn Any> = None;

    // allocate a buffer for communicating with the host
    let mut s: String<1024> = String::new();
    let mut buf = [0u8; 1024];
    let mut idx = 0;
    loop {
        // read as much as we can off the wire
        let count = unwrap!(usart.read_until_idle(&mut buf[idx..]).await);
        let zero_idx = if count == 0 {
            continue;
        } else {
            // calculate the index of zero in the buffer
            let zero_idx = buf[idx..idx + count]
                .iter()
                .position(|x| *x == 0)
                .map(|pos| pos + idx);

            // log that we managed to read some data
            info!(
                "Read {} bytes - {:02X} - {:?}",
                count,
                buf[idx..idx + count],
                zero_idx
            );

            // update state
            idx += count;

            zero_idx
        };

        if let Some(zi) = zero_idx {
            // attempt to parse as a message
            core::write!(
                s,
                "Parsed message: {:?}",
                postcard::from_bytes_cobs::<ClientMessage<K1>>(&mut buf[..=zi]).ok()
            )
            .ok();
            info!("{}", s.as_str());

            // reset the state
            // copy all the data we read after the 0 byte to the start of the buffer
            buf.copy_within(zi + 1..idx, 0);
            idx = idx.saturating_sub(zi + 1);
        }

        if idx == buf.len() {
            // we've reached the end of the buffer and haven't found a message
            // so reset the state by just clearing the buffer
            idx = 0;
            warn!("Weird state encountered - filled entire buffer without finding message.");
        }

        // send the message back
        // unwrap!(usart.write(&buf[..count]).await);
    }
}
