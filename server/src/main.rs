#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod database;

use aucpace::{AuCPaceServer, ClientMessage};
use core::fmt::Write as _;
use core::sync::atomic::{AtomicUsize, Ordering};
use database::SingleUserDatabase;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::interrupt::USART2;
use embassy_stm32::peripherals::{DMA1_CH5, DMA1_CH6};
use embassy_stm32::usart::{Config, Uart};
use embassy_stm32::{interrupt, peripherals, usart};
use embassy_time::Instant;
use heapless::String;
use rand_chacha::{ChaCha8Rng, ChaChaRng};
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
    let mut rng = ChaCha8Rng::seed_from_u64(now);
    info!("Seeded RNG - seed = {}", now);

    // create our AuCPace server
    let _base_server: AuCPaceServer<sha2::Sha512, _, K1> = AuCPaceServer::new(rng);

    // create something to receive messages
    let mut buf = [0u8; 1024];
    let mut receiver = MessageReceiver::new(&mut buf);

    let _msg: ClientMessage<K1> = receiver.receive_msg(&mut usart).await;
    loop {}

    // send the message back
    // unwrap!(usart.write(&buf[..count]).await);
}

// TODO: try just using a macro lmao, this shit sucks
async fn receive_msg<'a, const K1: usize>(
    buf: &'a mut [u8; 1024],
    usart: &mut Uart<'_, peripherals::USART2, DMA1_CH6, DMA1_CH5>,
) -> ClientMessage<'a, K1> {
    static IDX: AtomicUsize = AtomicUsize::new(0);

    let mut s: String<1024> = String::new();
    loop {
        // read as much as we can off the wire
        let remaining = &mut buf[IDX.load(Ordering::Relaxed)..];
        let count = unwrap!(usart.read_until_idle(remaining).await);
        drop(remaining);
        let zero_idx = if count == 0 {
            continue;
        } else {
            // calculate the index of zero in the buffer
            let zero_idx = buf[IDX.load(Ordering::Relaxed)..IDX.load(Ordering::Relaxed) + count]
                .iter()
                .position(|x| *x == 0)
                .map(|pos| pos + IDX.load(Ordering::Relaxed));

            // log that we managed to read some data
            info!(
                "Read {} bytes - {:02X} - {:?}",
                count,
                buf[IDX.load(Ordering::Relaxed)..IDX.load(Ordering::Relaxed) + count],
                zero_idx
            );

            // update state
            IDX.fetch_add(count, Ordering::Relaxed);

            zero_idx
        };

        let Some(zi) = zero_idx else {
            let swapped = IDX.compare_exchange(buf.len(), 0, Ordering::Relaxed, Ordering::Relaxed).is_ok();
            if swapped {
                warn!("Weird state encountered - filled entire buffer without finding message.");
            }

            continue;
        };

        let parsed = postcard::from_bytes_cobs::<ClientMessage<K1>>(&mut buf[..=zi]);
        let msg = match parsed {
            Ok(msg) => {
                core::write!(s, "Parsed message - {msg:?}").ok();
                info!("{}", s.as_str());
                msg
            }
            Err(e) => {
                core::write!(s, "Failed to parse message - {e:?}").ok();
                error!("{}", s.as_str());
                continue;
            }
        };

        // reset the state
        // copy all the data we read after the 0 byte to the start of the buffer
        buf.copy_within(zi + 1..IDX.load(Ordering::Relaxed), 0);
        IDX.load(Ordering::Relaxed) = IDX.load(Ordering::Relaxed).saturating_sub(zi + 1);

        // now return the parsed
        return msg;
    }
}
