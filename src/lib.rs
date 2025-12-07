#![no_std]
#![allow(incomplete_features)]
#![deny(warnings)]
#![feature(associated_type_defaults)]
#![feature(const_default)]
#![feature(const_trait_impl)]
#![feature(derive_const)]
#![feature(format_args_nl)]
#![feature(generic_const_exprs)]

pub mod dma;
#[macro_use]
pub mod debug;
pub mod i2c;
pub mod interrupt;
#[cfg(feature = "cpu_stm32h503")]
pub mod usb;
pub mod utils;
pub mod vcell;

use core::fmt::Arguments;

#[cfg(feature = "cpu_stm32g030")]
use stm32g030 as stm32;

#[cfg(feature = "cpu_stm32h503")]
use stm32h503 as stm32;

#[cfg(feature = "cpu_stm32u031")]
use stm32u031 as stm32;

use crate::vcell::UCell;

const DEBUG_ENABLE: bool = cfg!(feature = "internal_debug");

fn debug_fmt(fmt: Arguments) {
    if DEBUG_ENABLE {
        if let Some(f) = *DEBUG_HANDLER.as_ref() {
            f(fmt);
        }
    }
}

/// Set the handler for dbgln! uses within this crate.
///
/// SAFETY:  The user must ensure this is not called concurrently with any
/// dbgln! invocations from this crate.  Typically, do it once, before the rest
/// of the library is initialized and interrupts enabled.
#[inline]
pub unsafe fn set_debug_handler(f: Option<fn(Arguments)>) {
    if DEBUG_ENABLE {
        *unsafe {DEBUG_HANDLER.as_mut()} = f;
    }
}

static DEBUG_HANDLER: UCell<Option<fn(Arguments)>> = UCell::default();
