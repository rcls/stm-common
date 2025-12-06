use crate::utils::barrier;

#[allow(non_camel_case_types)]
pub trait DMA_Channel {
    /// Write to peripheral.  DADDR should be initialized.  The channel should
    /// be initialised by writes_t0().  Only a size of 0 (bytes) is currently
    /// supported.
    fn write(&self, data: usize, len: usize, size: u8);

    /// Read from peripheral. The channel should be initialized by read_from().
    /// Only as size of 0 (bytes) is currently supported.
    fn read(&self, data: usize, len: usize, size: u8);

    /// Configure to write to a peripheral from memory.
    fn writes_to(&self, dst: *mut   u8, request: u8);
    /// Configure to read from a peripheral to memory.
    fn read_from(&self, src: *const u8, request: u8);

    /// Stop and cancel an in-process transfer.
    fn abort(&self);

    /// Is the channel busy?
    #[cfg(feature = "cpu_stm32h503")]
    fn busy(&self) -> bool;
}

#[cfg(feature = "cpu_stm32h503")]
pub type Channel = stm32h503::gpdma1::c::C;

#[cfg(feature = "cpu_stm32u031")]
pub type Channel = stm32u031::dma1::ch::CH;

#[cfg(feature = "cpu_stm32h503")]
impl DMA_Channel for Channel {
    fn write(&self, data: usize, len: usize, _size: u8) {
        self.SAR().write(|w| w.SA().bits(data as u32));
        self.BR1.write(|w| w.BNDT().bits(len as u16));
        self.CR.write(|w| w.EN().set_bit().TCIE().set_bit());
    }
    fn read(&self, data: usize, len: usize, _size: u8) {
        self.DAR().write(|w| w.DA().bits(data as u32));
        self.BR1.write(|w| w.BNDT().bits(len as u16));
        self.CR.write(|w| w.EN().set_bit().TCIE().set_bit());
    }
    fn writes_to(&self, dst: *mut u8, request: u8) {
        self.DAR().write(|w| w.DA().bits(dst as u32));
        self.TR1.write(|w| w.SINC().set_bit());
        self.TR2.write(|w| w.REQSEL().bits(request));
    }
    fn read_from(&self, src: *const u8, request: u8) {
        self.SAR().write(|w| w.SA().bits(src as u32));
        self.TR1.write(|w| w.DINC().set_bit());
        self.TR2.write(|w| w.REQSEL().bits(request));
    }
    fn abort(&self) {
        if self.CR.read().EN().bit() {
            self.CR.write(|w| w.SUSP().set_bit());
            while !self.SR.read().SUSPF().bit() {}
            self.CR.write(|w| w.RESET().set_bit());
            self.FCR.write(|w| w.bits(!0));
        }
    }
    fn busy(&self) -> bool {
        self.CR.read().EN().bit()
    }
}

impl DMA_Channel for Channel {
    fn write(&self, data: usize, len: usize, size: u8) {
        setup(self, data, len, size, true)}

    fn read(&self, data: usize, len: usize, size: u8) {
        setup(self, data, len, size, false)}

    fn writes_to(&self, dst: *mut u8, request: u8) {
        self.read_from(dst, request);
    }
    fn read_from(&self, src: *const u8, request: u8) {
        self.PAR.write(|w| w.bits(src as u32));
        // For some reason unsigned_offset_from here leads to crashes.  So
        // do it by hand.
        let me = self as *const Self;
        let dma = unsafe {&*stm32u031::DMA1::ptr()};
        let ch0 = dma.CH(0) as *const Self;
        let index = (me.addr() - ch0.addr()) / size_of::<Self>();
        // dbgln!("DMA Index = {index}");
        let dmamux = unsafe {&*stm32u031::DMAMUX::ptr()};
        dmamux.CCR[index].write(|w| w.bits(request as u32));
    }

    fn abort(&self) {
        self.CR.write(|w| w);
    }
}

fn setup(ch: &Channel, data: usize, len: usize, size: u8, write: bool) {
    ch.MAR .write(|w| w.bits(data as u32));
    ch.NDTR.write(|w| w.bits(len as u32));
    barrier();
    ch.CR.write(
        |w|w.EN().set_bit().TCIE().set_bit().TEIE().set_bit().MINC().set_bit()
            .DIR().bit(write).PSIZE().bits(size).MSIZE().bits(size));
}

/// Trait Flat is used to check that we pass sane types to things that use DMA.
pub trait Flat {
    #[inline(always)]
    fn addr(&self) -> usize {(self as *const Self).addr()}
}

impl Flat for u8 {}
impl<const N: usize, T: Flat> Flat for [T; N] {}
impl Flat for i16 {}
impl Flat for u16 {}
impl Flat for u32 {}
impl Flat for [u8] {}
