#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod database;
use database::SingleUserDatabase;

use aucpace::{AuCPaceServer, ClientMessage};
use core::fmt::Write as _;
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

struct MessageReceiver<'a> {
    buf: &'a mut [u8; 1024],
    idx: usize,
}

impl<'a> MessageReceiver<'a> {
    fn new(buf: &'a mut [u8; 1024]) -> Self {
        Self { buf, idx: 0 }
    }

    async fn receive_msg<const K1: usize>(
        &'a mut self,
        usart: &mut Uart<'_, peripherals::USART2, DMA1_CH6, DMA1_CH5>,
    ) -> ClientMessage<'a, K1> {
        let mut s: String<1024> = String::new();
        loop {
            // read as much as we can off the wire
            let remaining = &mut self.buf[self.idx..];
            let count = unwrap!(usart.read_until_idle(remaining).await);
            drop(remaining);
            let zero_idx = if count == 0 {
                continue;
            } else {
                // calculate the index of zero in the buffer
                let zero_idx = self.buf[self.idx..self.idx + count]
                    .iter()
                    .position(|x| *x == 0)
                    .map(|pos| pos + self.idx);

                // log that we managed to read some data
                info!(
                    "Read {} bytes - {:02X} - {:?}",
                    count,
                    self.buf[self.idx..self.idx + count],
                    zero_idx
                );

                // update state
                self.idx += count;

                zero_idx
            };

            let Some(zi) = zero_idx else {
                if self.idx == self.buf.len() {
                    // we've reached the end of the buffer and haven't found a message
                    // so reset the state by just clearing the buffer
                    self.idx = 0;
                    warn!("Weird state encountered - filled entire buffer without finding message.");
                }

                continue;
            };

            let parsed = postcard::from_bytes_cobs::<ClientMessage<K1>>(&mut self.buf[..=zi]);
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
            self.buf.copy_within(zi + 1..self.idx, 0);
            self.idx = self.idx.saturating_sub(zi + 1);

            // now return the parsed
            return msg;
        }
    }
}
