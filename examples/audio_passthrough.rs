#![no_main]
#![no_std]

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;

use panic_semihosting as _;
use cortex_m_rt::entry;
use cortex_m::asm;

use daisy_bsp as daisy;

use daisy::hal;
use hal::prelude::*;

use daisy::pac;
use pac::interrupt;

use daisy::audio;
use daisy::led::Led;
use daisy::loggit;

// - static global state ------------------------------------------------------

static AUDIO_INTERFACE: Mutex<RefCell<Option<audio::Interface>>> = Mutex::new(RefCell::new(None));


// - entry point --------------------------------------------------------------

#[entry]
fn main() -> ! {

    // - board setup ----------------------------------------------------------

    let board = daisy::Board::take().unwrap();

    let dp = pac::Peripherals::take().unwrap();

    let ccdr = board.freeze_clocks(dp.PWR.constrain(),
                                   dp.RCC.constrain(),
                                   &dp.SYSCFG);

    let pins = board.split_gpios(dp.GPIOA.split(ccdr.peripheral.GPIOA),
                                 dp.GPIOB.split(ccdr.peripheral.GPIOB),
                                 dp.GPIOC.split(ccdr.peripheral.GPIOC),
                                 dp.GPIOD.split(ccdr.peripheral.GPIOD),
                                 dp.GPIOE.split(ccdr.peripheral.GPIOE),
                                 dp.GPIOF.split(ccdr.peripheral.GPIOF),
                                 dp.GPIOG.split(ccdr.peripheral.GPIOG),
                                 dp.GPIOH.split(ccdr.peripheral.GPIOH));

    let mut led_user = daisy::led::LedUser::new(pins.LED_USER);

    let i2c2_pins = (
        pins.WM8731.SCL.into_alternate_af4(),
        pins.WM8731.SDA.into_alternate_af4(),
    );

    let sai1_pins = (
        pins.WM8731.MCLK_A.into_alternate_af6(),
        pins.WM8731.SCK_A.into_alternate_af6(),
        pins.WM8731.FS_A.into_alternate_af6(),
        pins.WM8731.SD_A.into_alternate_af6(),
        pins.WM8731.SD_B.into_alternate_af6(),
    );

    let sai1_prec = ccdr
        .peripheral
        .SAI1
        .kernel_clk_mux(hal::rcc::rec::Sai1ClkSel::PLL3_P);

    let i2c2_prec = ccdr.peripheral.I2C2;

    let audio_interface = audio::Interface::init(&ccdr.clocks,
                                                 sai1_prec,
                                                 sai1_pins,
                                                 i2c2_prec,                      // added i2c init
                                                 i2c2_pins,
                                                 ccdr.peripheral.DMA1).unwrap();


    // - audio callback -------------------------------------------------------

    // handle callback with function pointer
    #[cfg(not(feature = "alloc"))]
    let audio_interface = {
        fn callback(_fs: f32, block: &mut audio::Block) {
            for frame in block {
                let (left, right) = *frame;
                *frame = (left, right);
            }
        }

        audio_interface.spawn(callback)
    };

    // handle callback with closure (needs alloc)
    #[cfg(any(feature = "alloc"))]
    let audio_interface = { audio_interface.spawn(move |fs, block| {
            for frame in block {
                let (left, right) = *frame;
                *frame = (left, right);
            }
        })
    };

    let audio_interface = match audio_interface {
        Ok(audio_interface) => audio_interface,
        Err(e) => {
            loggit!("Failed to start audio interface: {:?}", e);
            loop {}
        }
    };

    cortex_m::interrupt::free(|cs| {
        AUDIO_INTERFACE.borrow(cs).replace(Some(audio_interface));
    });


    // - main loop ------------------------------------------------------------

    let one_second = ccdr.clocks.sys_ck().0;

    loop {
        led_user.on();
        asm::delay(one_second);
        led_user.off();
        asm::delay(one_second);
    }
}


// - interrupts ---------------------------------------------------------------

/// interrupt handler for: dma1, stream1
#[interrupt]
fn DMA1_STR1() {
    cortex_m::interrupt::free(|cs| {
        if let Some(audio_interface) = AUDIO_INTERFACE.borrow(cs).borrow_mut().as_mut() {
            match audio_interface.handle_interrupt_dma1_str1() {
                Ok(()) => (),
                Err(e) => {
                    loggit!("Failed to handle interrupt: {:?}", e);
                }
            };
        }
    });
}
