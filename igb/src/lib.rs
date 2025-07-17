#![no_std]

use core::ptr::NonNull;

use log::debug;
pub use mac::{MacAddr6, MacStatus};
pub use trait_ffi::impl_extern_trait;

pub use crate::err::DError;
use crate::{
    descriptor::{AdvRxDesc, AdvTxDesc},
    ring::{DEFAULT_RING_SIZE, Ring, TxRing},
};

extern crate alloc;

mod err;
mod mac;
#[macro_use]
pub mod osal;
mod descriptor;
mod phy;
mod ring;

pub use futures::{Stream, StreamExt};
pub use ring::RxRing;

pub struct Igb {
    mac: mac::Mac,
    phy: phy::Phy,
    rx_ring_addrs: [usize; 16],
    tx_ring_addrs: [usize; 16],
}

impl Igb {
    pub fn new(iobase: NonNull<u8>) -> Result<Self, DError> {
        let mac = mac::Mac::new(iobase);
        let phy = phy::Phy::new(mac);

        Ok(Self {
            mac,
            phy,
            rx_ring_addrs: [0; 16],
            tx_ring_addrs: [0; 16],
        })
    }

    pub fn open(&mut self) -> Result<(), DError> {
        self.mac.disable_interrupts();

        self.mac.reset()?;

        self.mac.disable_interrupts();

        debug!("reset done");

        let link_mode = self.mac.link_mode().unwrap();
        debug!("link mode: {link_mode:?}");
        self.phy.power_up()?;

        self.setup_phy_and_the_link()?;

        self.mac.set_link_up();

        self.phy.wait_for_auto_negotiation_complete()?;
        debug!("Auto-negotiation complete");
        self.config_fc_after_link_up()?;

        self.init_stat();

        self.mac.enable_interrupts();

        self.mac.enable_rx();
        self.mac.enable_tx();

        Ok(())
    }

    pub fn new_rx_ring(&mut self) -> Result<RxRing, DError> {
        let mut ring: Ring<AdvRxDesc> = Ring::new(0, self.mac.iobase(), DEFAULT_RING_SIZE)?;
        ring.init()?;
        let mut ring = RxRing::new(ring);
        self.rx_ring_addrs[0] = ring.addr().as_ptr() as usize;
        Ok(ring)
    }

    pub fn new_tx_ring(&mut self) -> Result<TxRing, DError> {
        let mut ring: Ring<AdvTxDesc> = Ring::new(0, self.mac.iobase(), DEFAULT_RING_SIZE)?;
        ring.init()?;
        let mut ring = TxRing::new(ring);
        self.tx_ring_addrs[0] = ring.addr().as_ptr() as usize;
        Ok(ring)
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
        self.mac.read_mac().into()
    }

    pub fn check_vid_did(vid: u16, did: u16) -> bool {
        // This is a placeholder for actual VID/DID checking logic.
        // In a real implementation, this would check the device's
        // vendor ID and device ID against the provided values.
        vid == 0x8086 && [0x10C9, 0x1533].contains(&did)
    }

    pub fn status(&self) -> MacStatus {
        self.mac.status()
    }

    pub fn enable_loopback(&mut self) {
        self.mac.enable_loopback();
    }

    pub fn disable_loopback(&mut self) {
        self.mac.disable_loopback();
    }

    fn init_stat(&mut self) {
        //TODO
    }

    /// # Safety
    /// This function should only be called from the interrupt handler.
    /// It will handle the interrupt by acknowledging
    pub unsafe fn handle_interrupt(&mut self) {
        let msg = self.mac.interrupts_ack();
        debug!("Interrupt message: {msg:?}");
        if msg.queue_idx & 0x1 != 0 {
            let rx_ring = unsafe { &mut *(self.rx_ring_addrs[0] as *mut Ring<AdvRxDesc>) };
            rx_ring.clean();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Speed {
    Mb10,
    Mb100,
    Mb1000,
}
