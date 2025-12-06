use core::marker::PhantomData;

pub trait Meta {
    /// Main entry point to run at boot.
    fn main() -> !;
    fn bugger();
    /// Initial stack pointer to use at boot; this is normally the end of ram.
    const INITIAL_SP: *const u8;
}

#[cfg(any(feature = "cpu_stm32u031", feature = "cpu_stm32g030"))]
pub const NUM_INTERRUPTS: usize = 32;

#[cfg(feature = "cpu_stm32h503")]
pub const NUM_INTERRUPTS: usize = 134;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct VectorTable<T> {
    pub stack     : *const u8,
    pub reset     : fn() -> !,
    pub nmi       : fn(),
    pub hard_fault: fn(),
    pub reserved1 : [u32; 7],
    pub svcall    : fn(),
    pub reserved2 : [u32; 2],
    pub pendsv    : fn(),
    pub systick   : fn(),
    pub isr       : [fn(); NUM_INTERRUPTS],
    pub phantom   : PhantomData<T>,
}

/// !@#$!@$#
unsafe impl<T> Sync for VectorTable<T> {}

impl<T: Meta> const Default for VectorTable<T> {
    fn default() -> Self {
        VectorTable{
            stack     : T::INITIAL_SP,
            reset     : T::main,
            nmi       : T::bugger,
            hard_fault: T::bugger,
            reserved1 : [0; 7],
            svcall    : T::bugger,
            reserved2 : [0; 2],
            pendsv    : T::bugger,
            systick   : T::bugger,
            isr       : [T::bugger; NUM_INTERRUPTS],
            phantom   : PhantomData
        }
    }
}
