#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod database;

use aucpace::{AuCPaceServer, ClientMessage, Database};
use core::fmt::Write as _;
use core::sync::atomic::{AtomicUsize, Ordering};
use curve25519_dalek::ristretto::CompressedRistretto;
use curve25519_dalek::RistrettoPoint;
use database::SingleUserDatabase;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::interrupt::USART2;
use embassy_stm32::usart::{Config, Uart, UartRx};
use embassy_stm32::{interrupt, peripherals, usart};
use embassy_time::Instant;
use heapless::String;
use rand_chacha::{ChaCha8Rng, ChaChaRng};
use rand_core::{RngCore, SeedableRng};
use sha2::digest::typenum::Compare;
use {defmt_rtt as _, panic_probe as _};

const K1: usize = 16;
const RECV_BUF_LEN: usize = 1024;

/// function like macro to wrap sending data over USART2, returns the number of bytes sent
macro_rules! send {
    ($recvr:ident, $buf:ident, $msg:ident) => {{
        let serialised = unwrap!(postcard::to_slice_cobs(&$msg, &mut $buf));
        $recvr.write(&serialised).await;
        serialised.len()
    }};
}

/// function like macro to wrap receiving data over USART2
macro_rules! recv {
    ($recvr:ident, $buf:ident, $s:ident) => {
        loop {
            let parsed: postcard::Result<ClientMessage<K1>> = $recvr.recv_msg(&mut $buf).await;
            match parsed {
                Ok(msg) => {
                    core::write!($s, "Parsed message - {msg:?}").ok();
                    debug!("{}", $s.as_str());
                    $s.clear();
                    break msg;
                }
                Err(e) => {
                    core::write!($s, "Failed to parse message - {e:?}").ok();
                    error!("{}", $s.as_str());
                    $s.clear();
                }
            };
        }
    };
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());
    info!("Initialised peripherals.");

    // configure USART2 which goes over the USB port on this board
    let config = Config::default();
    let irq = interrupt::take!(USART2);
    let (mut tx, mut rx) =
        Uart::new(p.USART2, p.PA3, p.PA2, irq, p.DMA1_CH6, p.DMA1_CH5, config).split();
    info!("Configured USART2.");

    // configure the RNG, kind of insecure but this is just a demo and I don't have real entropy
    let now = Instant::now().as_micros();
    let mut rng = ChaCha8Rng::seed_from_u64(now);
    info!("Seeded RNG - seed = {}", now);

    // create our AuCPace server
    let mut base_server: AuCPaceServer<sha2::Sha512, _, K1> = AuCPaceServer::new(rng);
    let mut database: SingleUserDatabase<100> = SingleUserDatabase::default();
    info!("Created the AuCPace Server and the Single User Database");

    // create something to receive messages
    let mut buf = [0u8; 1024];
    let mut receiver = MsgReceiver::new(rx);
    let mut s: String<1024> = String::new();
    info!("Receiver and buffers set up");

    // wait for a user to register themselves
    info!("Waiting for a registration packet.");
    loop {
        let msg = recv!(receiver, buf, s);
        if let ClientMessage::Registration {
            username,
            salt,
            params,
            verifier,
        } = msg
        {
            if username.len() > 100 {
                error!("Attempted to register with a username thats too long.");
            } else {
                database.store_verifier(username, salt, None, verifier, params);
                core::write!(s, "Registered {:?}", core::str::from_utf8(username));
                error!("{}", s);
                s.clear();
                break;
            }
        }
    }

    // now do a key-exchange
    info!("Beginning AuCPace protocol");
    let (server, message) = base_server.begin();
    let mut bytes_sent = send!(rx, message);
    info!("Sent Nonce");

    // wait for the client nonce
    let mut client_message: ClientMessage<K1> = recv!(stream, buf);
    let server = if let ClientMessage::Nonce(client_nonce) = client_message {
        server.agree_ssid(client_nonce)
    } else {
        panic!("Received invalid client message {:?}", client_message);
    };
    info!("Received Client Nonce");

    loop {}

    // send the message back
    // unwrap!(tx.write(&buf[..count]).await);
}

struct MsgReceiver<'uart> {
    buf: [u8; RECV_BUF_LEN],
    idx: usize,
    rx: UartRx<'uart, peripherals::USART2, peripherals::DMA1_CH5>,
}

impl<'uart> MsgReceiver<'uart> {
    fn new(rx: UartRx<'uart, peripherals::USART2, peripherals::DMA1_CH5>) -> Self {
        Self {
            buf: [0u8; 1024],
            idx: 0,
            rx,
        }
    }

    async fn recv_msg<'a>(
        &mut self,
        msg_buf: &'a mut [u8; RECV_BUF_LEN],
    ) -> postcard::Result<ClientMessage<'a, K1>> {
        loop {
            // read as much as we can off the wire
            let count = unwrap!(self.rx.read_until_idle(&mut self.buf[self.idx..]).await);
            let zero_idx = if count == 0 {
                continue;
            } else {
                // calculate the index of zero in the self.buffer
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
                if self.idx == RECV_BUF_LEN {
                    self.idx = 0;
                    warn!("Weird state encountered - filled entire self.buffer without finding message.");
                }

                continue;
            };

            // copy out from our buffer into the receiving buffer
            msg_buf[..=zi].copy_from_slice(&self.buf[..=zi]);

            // reset the state
            // copy all the data we read after the 0 byte to the start of the self.buffer
            self.buf.copy_within(zi + 1..self.idx, 0);
            self.idx = self.idx.saturating_sub(zi + 1);

            // parse the result
            break postcard::from_bytes_cobs::<ClientMessage<K1>>(msg_buf);
        }
    }
}
