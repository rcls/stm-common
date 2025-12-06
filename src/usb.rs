
pub mod control;
pub mod hardware;
pub mod string;
pub mod types;

// use crate::cpu::{CPU_FREQ, interrupt, nothing};
use crate::usb::hardware::{
    CTRL_RX_OFFSET, CheprWriter, bd_control, chep_block, chep_ctrl};
use crate::usb::types::{SetupHeader, SetupResult};

macro_rules!ctrl_dbgln {($($tt:tt)*) => {if false {crate::dbgln!($($tt)*)}};}
macro_rules!usb_dbgln  {($($tt:tt)*) => {if false {crate::dbgln!($($tt)*)}};}
macro_rules!fast_dbgln {($($tt:tt)*) => {if false {crate::dbgln!($($tt)*)}};}

pub(crate) use {ctrl_dbgln, usb_dbgln};

pub trait EndpointPair: const Default {
    /// Handler for RX done notifications.
    fn rx_handler(&mut self) {}
    /// Handler for TX done notifications.
    fn tx_handler(&mut self) {}
    /// Start-of-frame handler.
    fn start_of_frame(&mut self) {}
    /// Do we want to handle a setup request?
    #[inline(always)]
    fn setup_wanted(&mut self, _h: &SetupHeader) -> bool {
        false
    }
    /// Handler for set-up requests.  Currently no RX data supported.
    fn setup_handler(&mut self, _h: &SetupHeader) -> SetupResult {
        SetupResult::error()
    }

    /// Hardware level initialization.
    fn initialize() {}
}

pub trait USBTypes: const Default {
    fn get_device_descriptor(&mut self) -> SetupResult;
    fn get_config_descriptor(&mut self, setup: &SetupHeader) -> SetupResult;
    fn get_string_descriptor(&mut self, idx: u8) -> SetupResult;

    type EP1: EndpointPair = DummyEndPoint;
    type EP2: EndpointPair = DummyEndPoint;
    type EP3: EndpointPair = DummyEndPoint;
    type EP4: EndpointPair = DummyEndPoint;
    type EP5: EndpointPair = DummyEndPoint;
    type EP6: EndpointPair = DummyEndPoint;
    type EP7: EndpointPair = DummyEndPoint;

    /// CPU frequency, in HZ.
    const CPU_FREQ: u32;
}

#[derive_const(Default)]
pub struct DummyEndPoint;
impl EndpointPair for DummyEndPoint {}

pub struct DataEndPoints<UT: USBTypes> {
    pub ep1: UT::EP1,
    pub ep2: UT::EP2,
    pub ep3: UT::EP3,
    pub ep4: UT::EP4,
    pub ep5: UT::EP5,
    pub ep6: UT::EP6,
    pub ep7: UT::EP7,
}

#[allow(non_camel_case_types)]
pub struct USB_State<UT: USBTypes> {
    /// Meta-data: descriptors etc.
    pub meta: UT,
    /// Last set-up received, while we are processing it.
    setup: SetupHeader,
    /// Set-up data to send.  On TX ACK we send the next block.
    setup_data: SetupResult,
    /// If set, the TX setup data is shorter than the requested data and we must
    /// end with a zero-length packet if needed.
    setup_short: bool,
    /// Address received in a SET ADDRESS.  On TX ACK, we apply this.
    pending_address: Option<u8>,
    /// Are we configured?
    configured: bool,
    /// Callback for post-setup OUT data.  We only support single packets!
    setup_rx_cb: Option<fn() -> bool>,
    /// Callback for post-setup IN data (or ACK) completion.
    setup_tx_cb: control::SetupTxCallback,

    pub ep1: UT::EP1,
    pub ep2: UT::EP2,
    pub ep3: UT::EP3,
    pub ep4: UT::EP4,
    pub ep5: UT::EP5,
    pub ep6: UT::EP6,
    pub ep7: UT::EP7,
}

impl<UT: USBTypes> const Default for DataEndPoints<UT> {
    fn default() -> Self {Self{
        ep1: UT::EP1::default(),
        ep2: UT::EP2::default(),
        ep3: UT::EP3::default(),
        ep4: UT::EP4::default(),
        ep5: UT::EP5::default(),
        ep6: UT::EP6::default(),
        ep7: UT::EP7::default(),
    }}
}

impl<UT: USBTypes + const Default> const Default for USB_State<UT> {
    fn default() -> Self {Self{
        meta: UT::default(),
        setup: SetupHeader::default(),
        setup_data: SetupResult::default(),
        setup_short: false,
        pending_address: None,
        configured: false,
        setup_rx_cb: None,
        setup_tx_cb: None,

        ep1: Default::default(),
        ep2: Default::default(),
        ep3: Default::default(),
        ep4: Default::default(),
        ep5: Default::default(),
        ep6: Default::default(),
        ep7: Default::default(),
    }}
}

unsafe impl<UT: USBTypes> Sync for USB_State<UT> {}

impl<UT: USBTypes> USB_State<UT> {
    pub fn init(&mut self) {
        let crs   = unsafe {&*stm32h503::CRS  ::ptr()};
        let gpioa = unsafe {&*stm32h503::GPIOA::ptr()};
        let rcc   = unsafe {&*stm32h503::RCC  ::ptr()};
        let usb   = unsafe {&*stm32h503::USB  ::ptr()};

        // Bring up the HSI48 clock.
        rcc.CR.modify(|_,w| w.HSI48ON().set_bit());
        while !rcc.CR.read().HSI48RDY().bit() {
        }
        // Route the HSI48 to USB.
        rcc.CCIPR4.modify(|_,w| w.USBFSSEL().B_0x3());

        // Configure pins (PA11, PA12).  (PA9 = VBUS?)
        gpioa.AFRH.modify(|_,w| w.AFSEL11().B_0xA().AFSEL12().B_0xA());
        gpioa.MODER.modify(|_,w| w.MODE11().B_0x2().MODE12().B_0x2());

        // Enable CRS and USB clocks.
        rcc.APB1LENR.modify(|_,w| w.CRSEN().set_bit());
        rcc.APB2ENR.modify(|_,w| w.USBFSEN().set_bit());

        // crs_sync_in_2 USB SOF selected - default.
        crs.CR.modify(|_,w| w.AUTOTRIMEN().set_bit());
        crs.CR.modify(|_,w| w.AUTOTRIMEN().set_bit().CEN().set_bit());

        usb.CNTR.write(|w| w.PDWN().clear_bit().USBRST().set_bit());
        // Wait t_startup (1µs).
        for _ in 0 .. UT::CPU_FREQ / 2000000 {
            crate::utils::nothing();
        }
        usb.CNTR.write(|w| w.PDWN().clear_bit().USBRST().clear_bit());
        usb.BCDR.write(|w| w.DPPU_DPD().set_bit());

        // Clear any spurious interrupts.
        usb.ISTR.write(|w| w);

        self.usb_initialize();
    }

    pub fn isr(&mut self) -> bool {
        let usb = unsafe {&*stm32h503::USB::ptr()};
        let mut istr = usb.ISTR.read();
        let not_only_sof = istr.RST_DCON().bit() || istr.CTR().bit();
        if not_only_sof {
            fast_dbgln!("*** USB isr ISTR = {:#010x} FN={}",
                        istr.bits(), usb.FNR.read().FN().bits());
        }
        // Write zero to the interrupt bits we wish to acknowledge.
        usb.ISTR.write(|w| w.bits(!istr.bits() & !0x37fc0));

        if istr.SOF().bit() {
            self.start_of_frame();
        }

        if istr.RST_DCON().bit() {
            self.usb_initialize();
        }

        while istr.CTR().bit() {
            if istr.DIR().bit() {
                Self::errata_delay();
            }
            match istr.bits() & 31 {
                0  => self.control_tx_handler(),
                1  => self.ep1.tx_handler(),
                2  => self.ep2.tx_handler(),
                3  => self.ep3.tx_handler(),
                4  => self.ep4.tx_handler(),
                5  => self.ep5.tx_handler(),
                6  => self.ep6.tx_handler(),
                7  => self.ep7.tx_handler(),
                16 => self.control_rx_handler(),
                17 => self.ep1.rx_handler(),
                18 => self.ep2.rx_handler(),
                19 => self.ep3.rx_handler(),
                20 => self.ep4.rx_handler(),
                21 => self.ep5.rx_handler(),
                22 => self.ep6.rx_handler(),
                23 => self.ep7.rx_handler(),
                _  => {
                    dbgln!("Bugger endpoint?, ISTR = {:#010x}", istr.bits());
                    break;  // FIXME, this will hang!
                },
            }
            istr = usb.ISTR.read();
        }

        if not_only_sof {
            fast_dbgln!("CHEP0 now {:#010x}\n***", chep_ctrl().read().bits());
        }

        not_only_sof
    }

    /// On a start-of-frame interrupt, if the serial IN end-point is idle, we
    /// push through any pending data.  Hopefully quickly enough for the actual
    /// IN request.
    fn start_of_frame(&mut self) {
        self.ep1.start_of_frame();
        self.ep2.start_of_frame();
        self.ep3.start_of_frame();
        self.ep4.start_of_frame();
        self.ep5.start_of_frame();
        self.ep6.start_of_frame();
        self.ep7.start_of_frame();
    }

    fn usb_initialize(&mut self) {
        let usb = unsafe {&*stm32h503::USB::ptr()};
        usb_dbgln!("USB initialize...");

        self.control_initialize();

        usb.CNTR.write(
            |w|w.PDWN().clear_bit().USBRST().clear_bit()
                .RST_DCONM().set_bit().CTRM().set_bit().SOFM().set_bit());

        usb.DADDR.write(|w| w.EF().set_bit().ADD().bits(0));

        bd_control().rx.write(chep_block::<64>(CTRL_RX_OFFSET));

        let ctrl = chep_ctrl().read();
        chep_ctrl().write(
            |w| w.control().dtogrx(&ctrl, false).dtogtx(&ctrl, false)
                 .rx_valid(&ctrl));

        Self::ep_initialize();
    }

    /// Initialize all the RX BD entries, except for the control ones.
    fn ep_initialize() {
        UT::EP1::initialize();
        UT::EP2::initialize();
        UT::EP3::initialize();
        UT::EP4::initialize();
        UT::EP5::initialize();
        UT::EP6::initialize();
        UT::EP7::initialize();
    }

    fn errata_delay() {
        // ERRATA:
        //
        // During OUT transfers, the correct transfer interrupt (CTR) is
        // triggered a little before the last USB SRAM accesses have completed.
        // If the software responds quickly to the interrupt, the full buffer
        // contents may not be correct.
        //
        // Workaround: Software should ensure that a small delay is included
        // before accessing the SRAM contents. This delay should be
        // 800 ns in Full Speed mode and 6.4 μs in Low Speed mode.
        for _ in 0 .. UT::CPU_FREQ / 1250000 / 2 {
            crate::utils::nothing();
        }
    }
}
