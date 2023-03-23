#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod database;

use aucpace::{AuCPaceServer, ClientMessage, Database};
use core::fmt::Write as _;
use database::SingleUserDatabase;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::usart::{Config, Parity, Uart, UartRx};
use embassy_stm32::{interrupt, peripherals};
use embassy_time::Instant;
use heapless::String;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use {defmt_rtt as _, panic_probe as _};

const K1: usize = 16;
const RECV_BUF_LEN: usize = 1024;

/// Writing to a heapless::String then sending and clearing is annoying
macro_rules! fmt_log {
    (ERROR, $s:ident, $($arg:tt)*) => {
        core::write!($s, $($arg)*).ok();
        defmt::error!("{}", $s.as_str());
        $s.clear();
    };
    (WARN, $s:ident, $($arg:tt)*) => {
        core::write!($s, $($arg)*).ok();
        defmt::warn!("{}", $s.as_str());
        $s.clear();
    };
    (INFO, $s:ident, $($arg:tt)*) => {
        core::write!($s, $($arg)*).ok();
        defmt::info!("{}", $s.as_str());
        $s.clear();
    };
    (DEBUG, $s:ident, $($arg:tt)*) => {
        core::write!($s, $($arg)*).ok();
        defmt::debug!("{}", $s.as_str());
        $s.clear();
    };
    (TRACE, $s:ident, $($arg:tt)*) => {
        core::write!($s, $($arg)*).ok();
        defmt::trace!("{}", $s.as_str());
        $s.clear();
    };
}

/// function like macro to wrap sending data over USART2, returns the number of bytes sent
macro_rules! send {
    ($tx:ident, $buf:ident, $msg:ident) => {{
        let serialised = postcard::to_slice_cobs(&$msg, &mut $buf).unwrap();
        unwrap!($tx.write(&serialised).await);
        serialised.len()
    }};
}

/// function like macro to wrap receiving data over USART2
macro_rules! recv {
    ($recvr:ident, $s:ident) => {
        loop {
            let parsed = $recvr.recv_msg().await;
            match parsed {
                Ok(msg) => {
                    fmt_log!(DEBUG, $s, "Parsed message - {msg:?}");
                    break msg;
                }
                Err(e) => {
                    fmt_log!(ERROR, $s, "Failed to parse message - {e:?}");
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
    let server_rng = ChaCha8Rng::seed_from_u64(now);
    info!("Seeded RNG - seed = {}", now);

    // create our AuCPace server
    let mut base_server: AuCPaceServer<sha2::Sha512, _, K1> = AuCPaceServer::new(server_rng);
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
        let msg = recv!(receiver, s);
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
                fmt_log!(ERROR, s, "Registered {:?}", core::str::from_utf8(username));
                break;
            }
        }
    }

    loop {
        let now = Instant::now().as_micros();
        let mut session_rng = ChaCha8Rng::seed_from_u64(now);
        info!("Seeded Session RNG - seed = {}", now);

        // now do a key-exchange
        info!("Beginning AuCPace protocol");
        let (server, message) = base_server.begin();
        let mut bytes_sent = send!(tx, buf, message);
        info!("Sent Nonce");

        // ===== SSID Establishment =====
        let mut client_message: ClientMessage<K1> = recv!(receiver, s);
        let server = if let ClientMessage::Nonce(client_nonce) = client_message {
            server.agree_ssid(client_nonce)
        } else {
            fmt_log!(
                ERROR,
                s,
                "Received invalid client message {:?} - restarting negotiation",
                client_message
            );
            continue;
        };
        info!("Received Client Nonce");

        // ===== Augmentation Layer =====
        client_message = recv!(receiver, s);
        let (server, message) = if let ClientMessage::Username(username) = client_message {
            server.generate_client_info(username, &database, &mut session_rng)
        } else {
            fmt_log!(
                ERROR,
                s,
                "Received invalid client message {:?} - restarting negotiation",
                client_message
            );
            continue;
        };
        info!("Received Client Username");

        bytes_sent += send!(tx, buf, message);
        info!("Sent AugmentationInfo");

        info!("Total bytes sent: {}", bytes_sent);
    }
}

struct MsgReceiver<'uart> {
    buf: [u8; RECV_BUF_LEN],
    idx: usize,
    rx: UartRx<'uart, peripherals::USART2, peripherals::DMA1_CH5>,
    reset_pos: Option<usize>,
}

impl<'uart> MsgReceiver<'uart> {
    fn new(rx: UartRx<'uart, peripherals::USART2, peripherals::DMA1_CH5>) -> Self {
        Self {
            buf: [0u8; 1024],
            idx: 0,
            rx,
            reset_pos: None,
        }
    }

    /// Performs a reset if one is required
    fn reset(&mut self) {}

    async fn recv_msg(&mut self) -> postcard::Result<ClientMessage<'_, K1>> {
        // reset the state
        // copy all the data we read after the 0 byte to the start of the self.buffer
        if let Some(zi) = self.reset_pos {
            self.buf.copy_within(zi + 1..self.idx, 0);
            self.idx = self.idx.saturating_sub(zi + 1);
            self.reset_pos = None;
        }

        loop {
            // read as much as we can off the wire
            let count = unwrap!(self.rx.read_until_idle(&mut self.buf[self.idx..]).await);
            let zero_idx = if count == 0 {
                continue;
            } else {
                // log that we managed to read some data
                info!(
                    "Read {} bytes - {:02X}",
                    count,
                    self.buf[self.idx..self.idx + count],
                );

                // update state
                self.idx += count;

                // calculate the index of zero in the self.buffer
                // it is tempting to optimise this to just what is read but more than one packet can
                // be read at once so the whole buffer needs to be searched to allow for this behaviour
                let zero_idx = self.buf[..self.idx].iter().position(|x| *x == 0);

                zero_idx
            };

            let Some(zi) = zero_idx else {
                if self.idx == RECV_BUF_LEN {
                    self.idx = 0;
                    warn!("Weird state encountered - filled entire self.buffer without finding message.");
                }

                continue;
            };
            debug!("self.buf[..self.idx] = {:02X}", self.buf[..self.idx]);
            info!(
                "Found zero byte at index {} - {} - {}",
                zi, self.buf[zi], self.idx
            );

            // store zi for next time
            self.reset_pos = Some(zi);

            // parse the result
            break postcard::from_bytes_cobs::<ClientMessage<K1>>(&mut self.buf[..=zi]);
        }
    }
}
