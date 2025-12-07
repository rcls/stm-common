//! UART debug output.  This drives a STM32 serial port.  This is designed to
//! be used another crate directly, rather than being contained in our own
//! crate.
//!
//! We assume that the crate we are part of contains a few things...

use crate::utils::{WFE, barrier};
use crate::vcell::{UCell, VCell};

use core::fmt::{Arguments, Result};
use core::marker::PhantomData;

#[cfg(not(feature = "debug_lpuart"))]
pub type UART = crate::stm32::usart1::RegisterBlock;

#[cfg(feature = "debug_lpuart")]
pub type UART = crate::stm32::lpuart1::RegisterBlock;

pub trait Meta: Sized + 'static {
    fn debug() -> &'static Debug<Self>;
    fn uart(&self) -> &'static UART;
    fn lazy_init(&self);
    fn is_init(&self) -> bool;
    fn interrupt(&self) -> u32;

    const ENABLE: bool = true;
}

pub struct Debug<M> {
    pub w: VCell<u8>,
    pub r: VCell<u8>,
    buf: [UCell<u8>; 256],
    meta: M,
}

#[derive(Default)]
pub struct Marker<M> {
    meta: PhantomData<M>,
}

impl<M: const Default> const Default for Debug<M> {
    fn default() -> Debug<M> {
        Debug {
            w: VCell::new(0), r: VCell::new(0),
            buf: [const {UCell::new(0)}; 256],
            meta: M::default(),
        }
    }
}

impl<M: Meta> Debug<M> {
    pub fn write_bytes(&self, s: &[u8]) {
        if !M::ENABLE {
            return;
        }
        self.meta.lazy_init();
        let mut w = self.w.read();
        for &b in s {
            while self.r.read().wrapping_sub(w) == 1 {
                self.enable(w);
                self.push();
            }
            // SAFETY: The ISR won't access the array element in question.
            unsafe {*self.buf[w as usize].as_mut() = b};
            w = w.wrapping_add(1);
        }
        self.enable(w);
    }

    fn push(&self) {
        WFE();
        // If the interrupt is pending, call the ISR ourselves.  Read the bit
        // twice in case there is a race condition where we read pending on an
        // enabled interrupt.
        let nvic = unsafe {&*cortex_m::peripheral::NVIC::PTR};
        let interrupt = self.meta.interrupt();
        let bit: usize = interrupt as usize % 32;
        let idx: usize = interrupt as usize / 32;
        if nvic.icpr[idx].read() & 1 << bit == 0 {
            return;
        }
        // It might take a couple of goes for the pending state to clear, so
        // loop.
        while nvic.icpr[idx].read() & 1 << bit != 0 {
            unsafe {nvic.icpr[idx].write(1 << bit)};
            self.isr();
        }
    }

    fn enable(&self, w: u8) {
        barrier();
        self.w.write(w);

        let uart = self.meta.uart();
        // Use the FIFO empty interrupt.  Normally we should be fast enough
        // to refill before the last byte finishes.
        uart.CR1.write(
            |w| w.FIFOEN().set_bit().TE().set_bit().UE().set_bit()
                . TXFEIE().set_bit());
    }

    pub fn isr(&self) {
        if !M::ENABLE {
            return;
        }
        let uart = self.meta.uart();
        let sr = uart.ISR.read();
        if sr.TC().bit() {
            uart.CR1.modify(|_,w| w.TCIE().clear_bit());
        }
        if !sr.TXFE().bit() {
            return;
        }

        const FIFO_SIZE: usize = 8;
        let mut r = self.r.read() as usize;
        let w = self.w.read() as usize;
        let mut done = 0;
        while r != w && done < FIFO_SIZE {
            uart.TDR.write(|w| w.bits(*self.buf[r].as_ref() as u32));
            r = (r + 1) & 0xff;
            done += 1;
        }
        self.r.write(r as u8);
        if r == w {
            uart.CR1.modify(|_,w| w.TXFEIE().clear_bit());
        }
    }
}

pub fn flush<M: Meta>() {
    let debug = M::debug();
    if !M::ENABLE || !debug.meta.is_init() {
        return;                        // Not initialized, nothing to do.
    }

    let uart = debug.meta.uart();
    // Enable the TC interrupt.
    uart.CR1.modify(|_,w| w.TCIE().set_bit());
    // Wait for the TC bit.
    loop {
        let isr = uart.ISR.read();
        if debug.r.read() == debug.w.read()
            && isr.TC().bit() && isr.TXFE().bit() {
            break;
        }
        debug.push();
    }
}

#[inline]
pub fn write_str<M: Meta> (s: &str) {
    if M::ENABLE {
        M::debug().write_bytes(s.as_bytes());
    }
}

#[inline]
pub fn debug_fmt<M: Meta + Default> (fmt: Arguments<'_>) {
    if M::ENABLE {
        let _ = core::fmt::write(&mut Marker::<M>::default(), fmt);
    }
}

impl<M: Meta> core::fmt::Write for Marker<M> {
    #[inline]
    fn write_str(&mut self, s: &str) -> Result {
        write_str::<M>(s);
        Ok(())
    }
    #[inline]
    fn write_char(&mut self, c: char) -> Result {
        let cc = [c as u8];
        M::debug().write_bytes(&cc);
        Ok(())
    }
}

#[macro_export]
macro_rules! dbg {
    ($($tt:tt)*) => {if crate::DEBUG_ENABLE {
        crate::debug_fmt(format_args!($($tt)*));}}
}

#[macro_export]
macro_rules! dbgln {
    () => {if crate::DEBUG_ENABLE {
        crate::debug_fmt(format_args_nl!(""));}};
    ($($tt:tt)*) => {if crate::DEBUG_ENABLE {
        crate::debug_fmt(format_args_nl!($($tt)*));}};
}
