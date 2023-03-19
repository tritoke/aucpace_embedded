#![no_main]
#![no_std]

use panic_probe as _;
use defmt_rtt as _;

#[rtic::app(device = stm32f4xx_hal::pac, dispatchers = [EXTI1])]
mod app {
    use stm32f4xx_hal::{
        pac,
        prelude::*,
        timer::MonoTimerUs,
        serial,
    };
    use cortex_m::asm;
    use core::fmt::Write as _;
    use core::sync::atomic::{AtomicU32, Ordering};
    use defmt::info;

    #[monotonic(binds = TIM2, default = true)]
    type MicrosecMono = MonoTimerUs<pac::TIM2>;

    #[shared]
    struct Shared {
        ser_tx: serial::Tx<pac::USART2, u8>,
    }

    #[local]
    struct Local {
    }

    #[init]
    fn init(mut ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        let rcc = ctx.device.RCC.constrain();
        let clocks = rcc.cfgr.sysclk(48.MHz()).freeze();
        let mono = ctx.device.TIM2.monotonic_us(&clocks);
        let gpioa = ctx.device.GPIOA.split();

        // default is 115200 bps, 8N1
        let ser_tx_pin = gpioa.pa2.into_alternate::<7>();
        let _ser_rx_pin = gpioa.pa3.into_alternate::<7>();
        let ser_cfg = serial::Config::default().wordlength_8();
        let mut ser_tx = serial::Serial::tx(ctx.device.USART2, ser_tx_pin, ser_cfg, &clocks).unwrap();

        // log some initial messages to ensure we are all okay
        info!("Starting up...");

        (
            Shared {
                ser_tx,
            },
            Local { },
            init::Monotonics(mono),
        )
    }

    #[task(binds = USART2, priority=5, shared=[ser_tx])]
    fn hello(cx: hello::Context) {
        let hello::SharedResources {
            mut ser_tx,
        } = cx.shared;

        info!("Hello from hello :)");

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        ser_tx.lock(|ser_tx| {
            write!(ser_tx, "Hello - counter={}", COUNTER.load(Ordering::SeqCst)).ok();
        });

        COUNTER.fetch_add(1, Ordering::SeqCst);
    }

    // Background task, runs whenever no other tasks are running
    // #[idle]
    // fn idle(_cx: idle::Context) -> ! {
    //     loop {
    //         // Wait for interrupt...
    //         asm::wfi();
    //     }
    // }

}
