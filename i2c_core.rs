use core::marker::PhantomData;

use stm_common::vcell::{UCell, VCell};
use stm_common::utils::{WFE, barrier};

use crate::dma::{DMA_Channel, Flat};

use super::{I2C, RX_MUXIN, TX_MUXIN, rx_channel, tx_channel};

pub type Result = core::result::Result<(), ()>;

#[derive_const(Default)]
pub struct I2cContext {
    pub outstanding: VCell<u8>,
    error: VCell<u8>,
    pending_len: VCell<usize>,
}

/// Marker struct to indicate that we are waiting upon an I2C transaction.
///
/// The phantoms make sure we bind the lifetime, with the correct mutability.
/// We would much rather just have a PhantomData of the correct reference type,
/// but then Wait would be different depending on the data in flight!
#[must_use]
#[derive(Default)]
pub struct Wait<'a>(PhantomData<(&'a [u8], &'a mut [u8])>);

pub static CONTEXT: UCell<I2cContext> = UCell::default();

pub const F_I2C: u8 = 1;
pub const F_DMA_RX: u8 = 2;
pub const F_DMA_TX: u8 = 4;

macro_rules!dbgln {($($tt:tt)*) => {if false {crate::dbgln!($($tt)*)}};}

pub fn i2c_isr() {
    let i2c = unsafe {&*I2C::ptr()};
    let context = unsafe {CONTEXT.as_mut()};

    let status = i2c.ISR.read();
    dbgln!("I2C ISR {:#x}", status.bits());
    let todo = *context.pending_len.as_mut();
    *context.pending_len.as_mut() = 0;

    if todo != 0 && status.TC().bit() {
        // Assume write -> read transition.
        dbgln!("I2C now read {todo} bytes [{:#x}]", status.bits());
        let cr2 = i2c.CR2.read();
        i2c.CR2.write(
            |w|w.NBYTES().bits(todo as u8).START().set_bit()
                .AUTOEND().set_bit().RD_WRN().set_bit()
                .SADD().bits(cr2.SADD().bits()));
    }
    else if status.STOPF().bit() {
        // FIXME - if we see a stop when waiting for the above, we'll hang.
        dbgln!("I2C STOPF");
        i2c.ICR.write(|w| w.STOPCF().set_bit());
        *context.outstanding.as_mut() &= !F_I2C;
    }
    else if status.ARLO().bit() || status.BERR().bit() || status.NACKF().bit() {
        dbgln!("I2C Error");
        i2c.ICR.write(
            |w| w.ARLOCF().set_bit().BERRCF().set_bit().NACKCF().set_bit());
        *context.outstanding.as_mut() = 0;
        *context.error.as_mut() = 1;
    }
    else {
        panic!("Unexpected I2C ISR {:#x} {:#x}", status.bits(),
               i2c.CR2.read().bits());
    }
    // Stop the ISR from prematurely retriggering.  Otherwise we may return
    // from the ISR before the update has propagated through the I2C subsystem,
    // leaving the interrupt line high.
    i2c.ISR.read();

    dbgln!("I2C ISR done, {}", context.outstanding.read());
}

impl I2cContext {
    fn read_reg_start(&self, addr: u8, reg: u8, data: usize, len: usize) {
        // Should only be called while I2C idle...
        let i2c = unsafe {&*I2C::ptr()};
        self.arm(F_I2C | F_DMA_RX);
        self.pending_len.write(len);

        // Synchronous I2C start for the reg ptr write.
        // No DMA write is active so the dma req. hopefully just gets ignored.
        i2c.CR2.write(
            |w| w.START().set_bit().SADD().bits(addr as u16).NBYTES().bits(1));
        i2c.TXDR.write(|w| w.bits(reg as u32));

        rx_channel().read(data, len, 0);
    }
    #[inline(never)]
    fn read_start(&self, addr: u8, data: usize, len: usize) {
        let i2c = unsafe {&*I2C::ptr()};

        rx_channel().read(data, len, 0);
        self.arm(F_I2C | F_DMA_RX);
        i2c.CR2.write(
            |w|w.START().set_bit().AUTOEND().bit(true).SADD().bits(addr as u16)
                .RD_WRN().set_bit().NBYTES().bits(len as u8));
    }
    #[inline(never)]
    fn write_reg_start(&self, addr: u8, reg: u8, data: usize, len: usize) {
        let i2c = unsafe {&*I2C::ptr()};

        self.arm(F_I2C | F_DMA_TX);
        i2c.CR2.write(
            |w| w.START().set_bit().AUTOEND().set_bit()
                . SADD().bits(addr as u16).NBYTES().bits(len as u8 + 1));
        i2c.TXDR.write(|w| w.TXDATA().bits(reg));
        tx_channel().write(data, len, 0);
    }
    #[inline(never)]
    fn write_start(&self, addr: u8, data: usize, len: usize, last: bool) {
        let i2c = unsafe {&*I2C::ptr()};

        self.arm(F_I2C | F_DMA_TX);
        i2c.CR2.write(
            |w| w.START().set_bit().AUTOEND().bit(last)
                . SADD().bits(addr as u16).NBYTES().bits(len as u8));
        tx_channel().write(data, len, 0);
    }

    #[inline(never)]
    fn write_read_start(&self, addr: u8, wdata: usize, wlen: usize,
                        rdata: usize, rlen: usize) {
        let i2c = unsafe {&*I2C::ptr()};
        tx_channel().write(wdata, wlen, 0);
        rx_channel().read (rdata, rlen, 0);
        self.pending_len.write(rlen);
        self.arm(F_I2C | F_DMA_TX | F_DMA_RX);
        i2c.CR2.write(
            |w|w.START().set_bit().SADD().bits(addr as u16)
                .NBYTES().bits(wlen as u8));
    }
    fn arm(&self, flags: u8) {
        self.error.write(0);
        self.outstanding.write(flags);
        barrier();
    }

    fn done(&self) -> bool {self.outstanding.read() == 0}
    fn wait(&self) {
        while !self.done() {
            WFE();
        }
        barrier();
        if self.error.read() != 0 {
            self.error_cleanup();
        }
    }
    fn error_cleanup(&self) {
        dbgln!("I2C error cleanup");
        let i2c = unsafe {&*I2C::ptr()};
        // Clean-up the DMA and reset the I2C.
        i2c.CR1.write(|w| w.PE().clear_bit());
        tx_channel().abort();
        rx_channel().abort();
        rx_channel().read_from(i2c.RXDR.as_ptr() as *const u8, RX_MUXIN);
        tx_channel().writes_to(i2c.TXDR.as_ptr() as *mut   u8, TX_MUXIN);
        i2c.CR1.write(
            |w|w.TXDMAEN().set_bit().RXDMAEN().set_bit().PE().set_bit()
                .NACKIE().set_bit().ERRIE().set_bit().TCIE().set_bit()
                .STOPIE().set_bit());
        barrier();
    }
}

impl<'a> Wait<'a> {
    pub fn new<T: ?Sized>(_ : &'a T) -> Self {Self::default()}
    pub fn defer(self) {core::mem::forget(self);}
    pub fn wait(self) -> Result {
        CONTEXT.wait();
        let result = CONTEXT.error.read();
        core::mem::forget(self);
        if result == 0 {Ok(())} else {Err(())}
    }
}

impl Drop for Wait<'_> {
    fn drop(&mut self) {let _ = CONTEXT.wait();}
}

pub fn write<T: Flat + ?Sized>(addr: u8, data: &T) -> Wait<'_> {
    CONTEXT.write_start(addr & !1, data.addr(), size_of_val(data), true);
    Wait::new(data)
}

pub fn write_reg<T: Flat + ?Sized>(addr: u8, reg: u8, data: &T) -> Wait<'_> {
    CONTEXT.write_reg_start(addr & !1, reg, data.addr(), size_of_val(data));
    Wait::new(data)
}

pub fn read<T: Flat + ?Sized>(addr: u8, data: &mut T) -> Wait<'_> {
    CONTEXT.read_start(addr | 1, data.addr(), size_of_val(data));
    Wait::new(data)
}

pub fn read_reg<T: Flat + ?Sized>(addr: u8, reg: u8, data: &mut T) -> Wait<'_> {
    CONTEXT.read_reg_start(addr | 1, reg, data.addr(), size_of_val(data));
    Wait::new(data)
}

pub fn write_read<'a, T: Flat + ?Sized, U: Flat + ?Sized>(
    addr: u8, wdata: &'a T, rdata: &'a mut U) -> Wait<'a> {
    CONTEXT.write_read_start(addr, wdata.addr(), size_of_val(wdata),
                             rdata.addr(), size_of_val(rdata));
    Wait::new(rdata)
}
