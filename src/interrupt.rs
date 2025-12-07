use crate::utils::unreachable;


#[cfg(any(feature = "cpu_stm32u031", feature = "cpu_stm32g030"))]
pub const NUM_INTERRUPTS: usize = 32;

#[cfg(feature = "cpu_stm32h503")]
pub const NUM_INTERRUPTS: usize = 134;

/// We don't use disabling interrupts to establish ownership, so no need for the
/// enable to be unsafe.
pub fn enable_all() {
    #[cfg(target_arch = "arm")]
    unsafe{cortex_m::interrupt::enable()}
}

pub fn disable_all() {
    #[cfg(target_arch = "arm")]
    cortex_m::interrupt::disable()
}

pub fn enable(n: crate::stm32::Interrupt) {
    let nvic = unsafe {&*cortex_m::peripheral::NVIC::PTR};
    let bit: usize = n as usize % 32;
    let idx: usize = n as usize / 32;
    crate::link_assert!(size_of_val(&nvic.iser[idx as usize]) == 4);
    unsafe {nvic.iser[idx].write(1u32 << bit)};
}

pub fn enable_priority(n: crate::stm32::Interrupt, p: u8) {
    let nvic = unsafe {&*cortex_m::peripheral::NVIC::PTR};
    crate::link_assert!(size_of_val(&nvic.ipr[n as usize]) == 1);
    unsafe {nvic.ipr[n as usize].write(p)};

    enable(n);
}

#[derive(Clone, Copy)]
#[derive_const(Default)]
#[repr(C)]
pub struct VectorTable {
    pub stack     : *const u8 = core::ptr::null(),
    pub reset     : fn() -> ! = unreachable,
    pub nmi       : fn() = || unreachable(),
    pub hard_fault: fn() = || unreachable(),
    pub reserved1 : [u32; 7] = [0; _],
    pub svcall    : fn() = || unreachable(),
    pub reserved2 : [u32; 2] = [0; _],
    pub pendsv    : fn() = || unreachable(),
    pub systick   : fn() = || unreachable(),
    pub isr       : [fn(); NUM_INTERRUPTS] = [|| unreachable(); _],
}
#[cfg(target_os = "none")]
const _: () = const {assert!(core::mem::offset_of!(VectorTable, isr) == 64)};

/// !@#$!@$#
unsafe impl Sync for VectorTable {}

impl VectorTable {
    pub const fn new(stack: *const u8, reset: fn() -> !, bugger: fn())
            -> VectorTable {
        VectorTable{
            stack, reset,
            nmi       : bugger,
            hard_fault: bugger,
            svcall    : bugger,
            reserved1 : [0; _],
            reserved2 : [0; _],
            pendsv    : bugger,
            systick   : bugger,
            isr       : [bugger; _]}
    }
    pub const fn isr(&mut self,
                     n: crate::stm32::Interrupt, handler: fn()) -> &mut Self {
        self.isr[n as usize] = handler;
        self
    }
}
