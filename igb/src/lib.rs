#![no_std]

use core::{cell::RefCell, ptr::NonNull};

use log::debug;
pub use mac::{MacAddr6, MacStatus};
use tock_registers::interfaces::*;

pub use crate::err::DError;

extern crate alloc;

mod err;
mod mac;
#[macro_use]
pub mod osal;
mod phy;

pub struct Igb {
    mac: RefCell<mac::Mac>,
    phy: phy::Phy,
}

impl Igb {
    pub fn new(iobase: NonNull<u8>) -> Self {
        let mac = RefCell::new(mac::Mac::new(iobase));
        let phy = phy::Phy::new(mac.clone());
        Self { mac, phy }
    }

    pub fn open(&mut self) -> Result<(), DError> {
        self.mac.borrow_mut().disable_interrupts();

        self.mac.borrow_mut().reset()?;

        self.mac.borrow_mut().disable_interrupts();

        debug!("reset done");

        let link_mode = self.mac.borrow().link_mode().unwrap();
        debug!("link mode: {link_mode:?}");

        self.phy.power_up()?;
        self.setup_phy_and_the_link()?;

        self.init_stat();

        self.init_rx();
        self.init_tx();

        // self.enable_interrupts();

        self.mac.borrow_mut().set_link_up();
        self.phy.wait_for_auto_negotiation_complete()?;
        debug!("Auto-negotiation complete");

        Ok(())
    }

    fn setup_phy_and_the_link(&mut self) -> Result<(), DError> {
        self.phy.power_up()?;
        debug!("PHY powered up");
        self.phy.enable_auto_negotiation()?;

        Ok(())
    }

    pub fn read_mac(&self) -> MacAddr6 {
        self.mac.borrow().read_mac().into()
    }

    pub fn check_vid_did(vid: u16, did: u16) -> bool {
        // This is a placeholder for actual VID/DID checking logic.
        // In a real implementation, this would check the device's
        // vendor ID and device ID against the provided values.
        vid == 0x8086 && [0x10C9, 0x1533].contains(&did)
    }

    pub fn status(&self) -> MacStatus {
        self.mac.borrow().status()
    }

    fn init_stat(&mut self) {
        //TODO
    }
    /// 4.5.9 Receive Initialization
    fn init_rx(&mut self) {
        // disable rx when configing.
        // self.mac.borrow_mut().disable_rx();

        // self.rx_ring.init();

        // self.reg.write_reg(RCTL::RXEN | RCTL::SZ_4096);
        self.mac
            .borrow_mut()
            .reg_mut()
            .rctl
            .modify(mac::RCTL::RXEN::Enabled);
    }

    fn init_tx(&mut self) {
        // self.reg.write_reg(TCTL::empty());

        // self.tx_ring.init();

        // self.reg.write_reg(TCTL::EN);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Speed {
    Mb10,
    Mb100,
    Mb1000,
}
