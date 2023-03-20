use anyhow::{anyhow, Result};
use aucpace::Client;
use clap::Parser;
use serialport::SerialPortType;
use std::io::{ErrorKind, Read, Write};
use std::time::Duration;

const USART_BAUD: u32 = 115200;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the USB port to open
    #[arg(short, long)]
    port: Option<String>,

    /// List USB ports on the system
    #[arg(long)]
    list_ports: bool,

    /// Perform partial augmentation
    #[arg(long = "partial")]
    partial_aug: bool,

    /// Perform strong AuCPace
    #[arg(long)]
    strong: bool,

    /// Perform implicit mutual authentication instead of explicit mutual authentication
    #[arg(long)]
    implicit: bool,
}

fn main() -> Result<()> {
    let args = Args::try_parse()?;

    // list the ports if the user asks for it
    if args.list_ports {
        let mut ports = serialport::available_ports()?;
        ports.retain(|port| matches!(port.port_type, SerialPortType::UsbPort(_)));
        eprintln!("Found the following USB ports:");
        for port in ports {
            eprintln!("{}", port.port_name);
        }

        return Ok(());
    }

    // open the serial port connection
    let port_name = args
        .port
        .ok_or_else(|| anyhow!("Must supply a USB port."))?;
    let mut serial = serialport::new(port_name, USART_BAUD).open()?;
    serial.set_timeout(Duration::from_secs(1))?;

    // start the client
    let mut base_client = Client::new(rand_core::OsRng);
    let mut buf = [0u8; 1024];
    let (client, msg) = base_client.begin();
    let ser_msg = postcard::to_slice_cobs(&msg, &mut buf)?;
    serial.write_all(ser_msg)?;

    // send and receive messages :)
    let mut buf = [0u8; 1024];
    loop {
        match serial.read(&mut buf) {
            Ok(count) => {
                eprintln!(
                    "Received - {:02X?} :: {:?}",
                    &buf[..count],
                    String::from_utf8_lossy(&buf[..count])
                );
            }
            Err(e) => {
                if e.kind() != ErrorKind::TimedOut {
                    eprintln!("Encountered unrecognised error - e={e:?}");
                }
            }
        };

        serial.write(b"Beans")?;
        eprintln!("Sent - \"Beans\"");
    }

    Ok(())
}
