
use crate::utils::barrier;

use super::{USBTypes, ctrl_dbgln, usb_dbgln};
use super::types::*;
use super::hardware::*;

use crate::usb::EndpointPair;

pub type SetupTxCallback = Option<fn(&SetupHeader)>;

impl<UT: USBTypes> super::USB_State<UT> {
    pub fn control_tx_handler(&mut self) {
        let chep = chep_ctrl().read();
        ctrl_dbgln!("Control TX handler CHEP0 = {:#010x}", chep.bits());

        if !chep.VTTX().bit() {
            ctrl_dbgln!("Bugger!");
            return;
        }

        if let SetupResult::Tx(data, cb) = self.setup_data {
            self.setup_next_data(data, cb);
            chep_ctrl().write(
                |w| w.control().VTTX().clear_bit().tx_valid(&chep));
            return;
        }

        if let Some(cb) = self.setup_tx_cb {
            self.setup_tx_cb = None;
            cb(&self.setup);
            self.setup = SetupHeader::default();
        }

        chep_ctrl().write(
            |w|w.control().VTTX().clear_bit().rx_valid(&chep).dtogrx(&chep, false));
    }

    pub fn control_rx_handler(&mut self) {
        let chep = chep_ctrl().read();
        ctrl_dbgln!("Control RX handler CHEP0 = {:#010x}", chep.bits());

        if !chep.VTRX().bit() {
            ctrl_dbgln!("Bugger");
            return;
        }

        if !chep.SETUP().bit() {
            ctrl_dbgln!("Control RX handler, CHEP0 = {:#010x}, non-setup",
                        chep.bits());

            if self.setup.length == 0 {
                // Either it's an ACK to our data, or we weren't expecting this.
                // Just drop it and flush any outgoing data.
                self.setup_data = SetupResult::default();
                chep_ctrl().write(
                    |w|w.control().VTRX().clear_bit().rx_valid(&chep)
                        .stat_tx(&chep, 2));
                return;
            }

            let ok = self.setup_rx_data();
            self.setup = SetupHeader::default();
            // Send either a zero-length ACK or an error stall.
            bd_control().tx.write(chep_bd_tx(CTRL_TX_OFFSET, 0));
            chep_ctrl().write(
                |w|w.control().VTRX().clear_bit()
                    .stat_tx(&chep, if ok {3} else {1})
                    .rx_valid(&chep));
            return;
        }

        // The USBSRAM only supports 32 bit accesses.  However, that only makes
        // a difference to the AHB bus on writes, not reads.  So access the
        // setup packet in place.
        barrier();
        let setup = unsafe {SetupHeader::from_ptr(CTRL_RX_BUF)};
        self.setup = setup;

        let result = self.setup_rx_handler(&setup);
        match result {
            SetupResult::Tx(data, cb) => self.setup_send_data(&setup, data, cb),
            SetupResult::Rx(len, cb)
                if len == setup.length as usize && len != 0 => {
                // Receive some data (if len != 0).  TODO: is the length match
                // guarenteed?
                self.setup_rx_cb = cb;
                chep_ctrl().write(
                    |w|w.control().VTRX().clear_bit().rx_valid(&chep)
                        .dtogrx(&chep, true) //.dtogtx(&chep, true)
                );
                ctrl_dbgln!("Set-up data rx armed {len}, CHEP = {:#x}",
                            chep_ctrl().read().bits());
            },
            SetupResult::Rx(_, _) => {
                ctrl_dbgln!("Set-up error");
                self.setup = SetupHeader::default();
                // Set STATTX to 1 (stall).  FIXME - clearing DTOGRX should not
                // be needed.  FIXME - do we really want to stall TX, or just
                // NAK?
                chep_ctrl().write(
                    |w|w.control().VTRX().clear_bit()
                        .stat_rx(&chep, 1).stat_tx(&chep, 1));
            },
        }
    }

    pub fn control_initialize(&mut self) {
        *self = Self::default();
    }

    fn setup_rx_handler(&mut self, setup: &SetupHeader)
            -> SetupResult {
        // Cancel any pending set-address and set-up data.
        self.pending_address = None;
        self.setup_data = SetupResult::default();
        self.setup_rx_cb = None;

        let bd = bd_control().rx.read();
        let len = bd >> 16 & 0x03ff;
        if len < 8 {
            ctrl_dbgln!("Rx setup len = {len} < 8");
            return SetupResult::error();
        }
        ctrl_dbgln!("Rx setup {:02x} {:02x} {:02x} {:02x} -> {}",
               setup.request_type, setup.request, setup.value_lo, setup.value_hi,
               setup.length);
        match (setup.request_type, setup.request) {
            (0x80, 0x00) => SetupResult::tx_data(&0u16), // Status.
            (0x00, 0x05) => self.set_address(setup), // Set address.
            (0x80, 0x06) => match setup.value_hi { // Get descriptor.
                1 => self.meta.get_device_descriptor(),
                2 => self.meta.get_config_descriptor(setup),
                3 => self.meta.get_string_descriptor(setup.value_lo),
                // 6 => setup_result(), // Device qualifier.
                desc => {
                    usb_dbgln!("Unsupported get descriptor {desc}");
                    SetupResult::error()
                }
            },
            (0x00, 0x09) => self.set_configuration(setup.value_lo),
            // We enable our only config when we get an address, so we can
            // just ACK the set interface message.
            (0x01, 0x0b) => SetupResult::no_data(), // Set interface

            _ => {
                if self.ep1.setup_wanted(setup) {
                    return self.ep1.setup_handler(setup);
                }
                if self.ep2.setup_wanted(setup) {
                    return self.ep2.setup_handler(setup);
                }
                if self.ep3.setup_wanted(setup) {
                    return self.ep3.setup_handler(setup);
                }
                if self.ep4.setup_wanted(setup) {
                    return self.ep4.setup_handler(setup);
                }
                if self.ep5.setup_wanted(setup) {
                    return self.ep5.setup_handler(setup);
                }
                if self.ep6.setup_wanted(setup) {
                    return self.ep6.setup_handler(setup);
                }
                if self.ep7.setup_wanted(setup) {
                    return self.ep7.setup_handler(setup);
                }
                usb_dbgln!("Unknown setup {:02x} {:02x} {:02x} {:02x} -> {}",
                           setup.request_type, setup.request,
                           setup.value_lo, setup.value_hi, setup.length);
                SetupResult::error()
            },
        }
    }

    /// Process just received setup OUT data.
    fn setup_rx_data(&mut self) -> bool {
        // First check that we really were expecting data.
        let Some(cb) = self.setup_rx_cb else {return false};
        self.setup_rx_cb = None;
        cb()
    }

    // Note that data should be a tx_data or no_data.
    fn setup_send_data(&mut self, setup: &SetupHeader,
                       data: &'static [u8], cb: SetupTxCallback) {
        self.setup_short = data.len() < setup.length as usize;
        let len = if self.setup_short {data.len()} else {setup.length as usize};
        ctrl_dbgln!("Setup response length = {} -> {}", data.len(), len);

        self.setup_next_data(&data[..len], cb);

        let chep = chep_ctrl().read();
        chep_ctrl().write(|w| w.control().VTRX().clear_bit().tx_valid(&chep));
    }

    /// Send the next data from the control state.
    fn setup_next_data(&mut self, data: &'static [u8], cb: SetupTxCallback) {
        let len = data.len();
        let is_short = len < 64;
        let len = if is_short {len} else {64};
        ctrl_dbgln!("Setup TX {len} of {}", data.len());

        // Copy the data into the control TX buffer.
        unsafe {copy_by_dest32(data.as_ptr(), CTRL_TX_BUF, len)};

        if len != data.len() || !is_short && self.setup_short {
            self.setup_data = SetupResult::Tx(&data[len..], cb);
        }
        else {
            self.setup_data = SetupResult::default();
            self.setup_tx_cb = cb;
        }

        // If the length is zero, then we are sending an ack.  If the length
        // is non-zero, then we are sending data and expect an ack.
        bd_control().tx.write(chep_bd_tx(CTRL_TX_OFFSET, len));
    }

    fn set_address(&mut self, header: &SetupHeader) -> SetupResult {
        usb_dbgln!("Set addr received {}", header.value_lo);
        SetupResult::no_data_cb(Self::do_set_address)
    }

    fn do_set_address(setup: &SetupHeader) {
        usb_dbgln!("Set address apply {}", setup.value_lo);
        let usb = unsafe {&*stm32h503::USB::ptr()};
        usb.DADDR.write(|w| w.EF().set_bit().ADD().bits(setup.value_lo));
    }

    fn set_configuration(&mut self, config: u8) -> SetupResult {
        if config == 0 {
            usb_dbgln!("Set configuration 0 - ignore");
        }
        else if config != 1 {
            usb_dbgln!("Set configuration {config} - error");
            return SetupResult::error();
        }
        else {
            usb_dbgln!("Set configuration {config}");
            super::USB_State::<UT>::ep_initialize();
            self.configured = true;
        }
        SetupResult::no_data()
    }
}
