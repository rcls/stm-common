//! UART debug output.  This drives a STM32 serial port.  This is designed to
//! be used another crate directly, rather than being contained in our own
//! crate.
//!
//! We assume that the crate we are part of contains a few things...

use crate::cpu::{WFE, barrier};
use crate::vcell::{UCell, VCell};

use super::{DEBUG, ENABLE, INTERRUPT, UART, lazy_init};

pub struct Debug {
    pub w: VCell<u8>,
    pub r: VCell<u8>,
    buf: [UCell<u8>; 256],
}

pub fn debug_isr() {
    if ENABLE {
        DEBUG.isr();
    }
}

impl const Default for Debug {
    fn default() -> Debug {
        Debug {
            w: VCell::new(0), r: VCell::new(0),
            buf: [const {UCell::new(0)}; 256]
        }
    }
}

impl Debug {
    pub fn write_bytes(&self, s: &[u8]) {
        if !ENABLE {
            return;
        }
        lazy_init();
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
        let bit: usize = INTERRUPT as usize % 32;
        let idx: usize = INTERRUPT as usize / 32;
        if nvic.icpr[idx].read() & 1 << bit == 0 {
            return;
        }
        // It might take a couple of goes for the pending state to clear, so
        // loop.
        while nvic.icpr[idx].read() & 1 << bit != 0 {
            unsafe {nvic.icpr[idx].write(1 << bit)};
            debug_isr();
        }
    }
    fn enable(&self, w: u8) {
        barrier();
        self.w.write(w);

        let uart = unsafe {&*UART::ptr()};
        // Use the FIFO empty interrupt.  Normally we should be fast enough
        // to refill before the last byte finishes.
        uart.CR1.write(
            |w| w.FIFOEN().set_bit().TE().set_bit().UE().set_bit()
                . TXFEIE().set_bit());
    }
    fn isr(&self) {
        let uart = unsafe {&*UART::ptr()};
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

pub fn flush() {
    if !super::is_init() {
        return;                        // Not initialized, nothing to do.
    }

    let uart = unsafe {&*UART::ptr()};
    // Enable the TC interrupt.
    uart.CR1.modify(|_,w| w.TCIE().set_bit());
    // Wait for the TC bit.
    loop {
        let isr = uart.ISR.read();
        if DEBUG.r.read() == DEBUG.w.read()
            && isr.TC().bit() && isr.TXFE().bit() {
            break;
        }
        DEBUG.push();
    }
}

pub fn write_str(s: &str) {
    DEBUG.write_bytes(s.as_bytes());
}

impl core::fmt::Write for super::DebugMarker {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_str(s);
        Ok(())
    }
    fn write_char(&mut self, c: char) -> core::fmt::Result {
        let cc = [c as u8];
        DEBUG.write_bytes(&cc);
        Ok(())
    }
}

#[macro_export]
macro_rules! dbg {
    ($($tt:tt)*) => {
        if $crate::debug::ENABLE {
            let _ = core::fmt::Write::write_fmt(
                &mut $crate::debug::debug_marker(), format_args!($($tt)*));
        }
    }
}

#[macro_export]
macro_rules! dbgln {
    () => {if $crate::debug::ENABLE {
        let _ = core::fmt::Write::write_str(
            &mut $crate::debug::debug_marker(), "\n");
        }};
    ($($tt:tt)*) => {if $crate::debug::ENABLE {
        let _ = core::fmt::Write::write_fmt(
            &mut $crate::debug::debug_marker(), format_args_nl!($($tt)*));
        }};
}
