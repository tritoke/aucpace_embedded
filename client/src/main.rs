use anyhow::{anyhow, Result};
use aucpace::{Client, ServerMessage};
use clap::Parser;
use scrypt::{Params, Scrypt};
use serialport::{DataBits, Parity, SerialPort, SerialPortType, StopBits};
use std::io::{ErrorKind, Read, Write};
use std::sync::Mutex;
use std::time::Duration;
use std::{io, thread};

#[allow(unused)]
use tracing::{debug, error, info, warn, Level};

const USART_BAUD: u32 = 115200;
const RECV_BUF_LEN: usize = 1024;
const K1: usize = 16;

/// function like macro to wrap sending data over the serial port, returns the number of bytes sent
macro_rules! send {
    ($serial_mtx:ident, $msg:ident) => {{
        let serialised = postcard::to_stdvec_cobs(&$msg).expect("Failed to serialise message");
        info!(
            "Sending {} byte long message - {serialised:02X?}",
            serialised.len()
        );
        $serial_mtx
            .lock()
            .expect("Failed to acquire serial port mutex")
            .write_all(&serialised)
            .expect("Failed to write message to serial");
        serialised.len()
    }};
}

/// function like macro to wrap receiving data over the serial port
macro_rules! recv {
    ($recvr:ident) => {
        loop {
            let parsed = $recvr.recv_msg();
            match parsed {
                Ok(msg) => {
                    debug!("Parsed message - {msg:?}");
                    break msg;
                }
                Err(e) => {
                    error!("Failed to parse message - {e:?}");
                }
            };
        }
    };
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the USB port to open
    #[arg(long)]
    port: Option<String>,

    /// List USB ports on the system
    #[arg(long)]
    list_ports: bool,

    /// Perform strong AuCPace
    #[arg(long)]
    strong: bool,

    /// Perform implicit mutual authentication instead of explicit mutual authentication
    #[arg(long)]
    implicit: bool,

    /// The Username to perform the exchange with
    #[arg(long, short)]
    username: String,

    /// The Password to perform the exchange with
    #[arg(long, short)]
    password: String,
}

fn main() -> Result<()> {
    let args = Args::try_parse()?;

    debug!("args={args:?}");

    // setup the logger
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_max_level(Level::DEBUG)
        .with_writer(io::stderr)
        .init();

    // list the ports if the user asks for it
    if args.list_ports {
        let mut ports = serialport::available_ports()?;
        ports.retain(|port| matches!(port.port_type, SerialPortType::UsbPort(_)));
        println!("Found the following USB ports:");
        for port in ports {
            println!("{}", port.port_name);
        }

        return Ok(());
    }

    // open the serial port connection
    let port_name = args
        .port
        .ok_or_else(|| anyhow!("Must supply a USB port."))?;
    let mut serial = Mutex::new({
        serialport::new(port_name, USART_BAUD)
            .timeout(Duration::from_millis(100))
            .open()?
    });
    let mut receiver = MsgReceiver::new(&serial);
    info!("Opened serial port connection.");

    // start the client
    let mut base_client = Client::new(rand_core::OsRng);

    let user = args.username.as_str();
    let pass = args.password.as_str();
    let message = base_client
        .register_alloc(user.as_bytes(), pass, Params::recommended(), Scrypt)
        .map_err(|e| anyhow!(e))?;
    send!(serial, message);
    info!("Registered as {user}:{pass}");

    info!("Starting AuCPace");
    let (client, message) = base_client.begin();
    send!(serial, message);

    // Receive the server's nonce
    let server_message = recv!(receiver);

    Ok(())
}

struct MsgReceiver<'mtx> {
    buf: [u8; RECV_BUF_LEN],
    idx: usize,
    mtx: &'mtx Mutex<Box<dyn SerialPort>>,
    reset_pos: Option<usize>,
}

impl<'mtx> MsgReceiver<'mtx> {
    fn new(mtx: &'mtx Mutex<Box<dyn SerialPort>>) -> Self {
        Self {
            buf: [0u8; 1024],
            idx: 0,
            mtx,
            reset_pos: None,
        }
    }

    fn recv_msg(&mut self) -> postcard::Result<ServerMessage<'_, K1>> {
        // reset the state
        // copy all the data we read after the 0 byte to the start of the self.buffer
        if let Some(zi) = self.reset_pos {
            self.buf.copy_within(zi + 1..self.idx, 0);
            self.idx = self.idx.saturating_sub(zi + 1);
            self.reset_pos = None;
        }

        // acquire a handle to the serial port
        let mut serial = self
            .mtx
            .lock()
            .expect("Failed to acquire lock for serial port.");

        loop {
            // read as much as we can off the wire
            let count = serial
                .read(&mut self.buf[self.idx..])
                .expect("Failed to read from serial port.");
            let zero_idx = if count == 0 {
                continue;
            } else {
                // update state
                self.idx += count;

                // calculate the index of zero in the self.buffer
                // it is tempting to optimise this to just what is read but more than one packet can
                // be read at once so the whole buffer needs to be searched to allow for this behaviour
                let zero_idx = self.buf[..self.idx].iter().position(|x| *x == 0);

                // log that we managed to read some data
                info!(
                    "Read {} bytes - {:02X?} - {:?}",
                    count,
                    &self.buf[self.idx..self.idx + count],
                    zero_idx
                );

                zero_idx
            };

            let Some(zi) = zero_idx else {
                if self.idx == RECV_BUF_LEN {
                    self.idx = 0;
                    warn!("Weird state encountered - filled entire self.buffer without finding message.");
                }

                continue;
            };

            self.reset_pos = Some(zi);
            // parse the result
            break postcard::from_bytes_cobs::<ServerMessage<K1>>(&mut self.buf[..=zi]);
        }
    }
}
