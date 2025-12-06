#![no_std]
#![feature(const_default)]
#![feature(const_trait_impl)]
#![feature(derive_const)]

pub mod dma;
#[macro_use]
pub mod debug_core;
pub mod utils;
pub mod vcell;

#[cfg(feature = "cpu_stm32h503")]
use stm32h503 as stm32;

#[cfg(feature = "cpu_stm32u031")]
use stm32u031 as stm32;
