#![no_std]

use core::{cell::RefCell, ptr::NonNull};

use log::debug;
pub use mac::{MacAddr6, MacStatus};
pub use trait_ffi::impl_extern_trait;

pub use crate::err::DError;
use crate::{
    descriptor::{AdvRxDesc, AdvTxDesc},
    ring::{DEFAULT_RING_SIZE, Ring},
};

extern crate alloc;

mod err;
mod mac;
#[macro_use]
pub mod osal;
mod descriptor;
mod phy;
mod ring;

pub struct Igb {
    mac: RefCell<mac::Mac>,
    phy: phy::Phy,
    tx_ring: Ring<AdvTxDesc>,
    rx_ring: Ring<AdvRxDesc>,
}

impl Igb {
    pub fn new(iobase: NonNull<u8>) -> Result<Self, DError> {
        let mac = RefCell::new(mac::Mac::new(iobase));
        let phy = phy::Phy::new(mac.clone());

        let tx_ring = Ring::new(0, iobase, DEFAULT_RING_SIZE)?;
        let rx_ring = Ring::new(0, iobase, DEFAULT_RING_SIZE)?;

        Ok(Self {
            mac,
            phy,
            tx_ring,
            rx_ring,
        })
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

        self.mac.borrow_mut().set_link_up();

        self.phy.wait_for_auto_negotiation_complete()?;
        debug!("Auto-negotiation complete");
        self.config_fc_after_link_up()?;

        self.init_stat();

        self.init_rx()?;
        self.init_tx();

        self.mac.borrow_mut().enable_interrupts();

        Ok(())
    }

    fn config_fc_after_link_up(&mut self) -> Result<(), DError> {
        // TODO 参考 drivers/net/ethernet/intel/igb/e1000_mac.c
        // igb_config_fc_after_link_up
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
    fn init_rx(&mut self) -> Result<(), DError> {
        // disable rx when configing.
        self.mac.borrow_mut().disable_rx();

        // 初始化 ring
        self.rx_ring.init()?;

        // 最后启用 RX
        self.mac.borrow_mut().enable_rx();

        Ok(())
    }

    fn init_tx(&mut self) {
        // self.mac.borrow_mut().reg_mut().tctl.write(mac::TCTL::empty());

        // self.tx_ring.init();

        // self.mac.borrow_mut().write_reg(TCTL::EN);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Speed {
    Mb10,
    Mb100,
    Mb1000,
}
