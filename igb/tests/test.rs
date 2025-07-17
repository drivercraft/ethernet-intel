#![no_std]
#![no_main]
#![feature(used_with_arg)]

use core::time::Duration;

use bare_test::time::spin_delay;
use eth_igb::{Stream, StreamExt, impl_trait, osal::Kernel};

extern crate alloc;
extern crate bare_test;

#[bare_test::tests]
mod tests {
    use core::{
        cell::UnsafeCell,
        ops::{Deref, DerefMut},
        time::Duration,
    };

    use bare_test::{
        fdt_parser::PciSpace,
        globals::{PlatformInfoKind, global_val},
        irq::{IrqHandleResult, IrqInfo, IrqParam},
        mem::iomap,
        platform::fdt::GetPciIrqConfig,
        println,
        time::spin_delay,
    };
    use eth_igb::Igb;
    use futures::StreamExt;
    use log::info;
    use pcie::{CommandRegister, PciCapability, RootComplexGeneric, SimpleBarAllocator};

    struct Driver<T>(UnsafeCell<T>);
    impl<T> Deref for Driver<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            unsafe { &*self.0.get() }
        }
    }
    impl<T> DerefMut for Driver<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            unsafe { &mut *self.0.get() }
        }
    }
    impl<T> Driver<T> {
        pub fn new(inner: T) -> Self {
            Self(UnsafeCell::new(inner))
        }
    }

    #[test]
    fn it_works() {
        let (mut igb, irq) = get_igb().unwrap();

        info!("igb: {:#?}", igb.status());

        let mut igb = Driver::new(igb);
        let igb_ptr = igb.0.get();

        for one in &irq.cfgs {
            IrqParam {
                intc: irq.irq_parent,
                cfg: one.clone(),
            }
            .register_builder({
                move |_irq| {
                    unsafe {
                        (*igb_ptr).handle_interrupt();
                    }
                    IrqHandleResult::Handled
                }
            })
            .register();
        }

        let mut rx = igb.new_rx_ring().unwrap();

        igb.open().unwrap();
        info!("igb opened: {:#?}", igb.status());

        info!("mac: {:#?}", igb.read_mac());

        info!("waiting for link up...");
        while !igb.status().link_up {
            spin_delay(Duration::from_secs(1));

            info!("status: {:#?}", igb.status());
        }

        spin_on::spin_on(async move {
            info!("link up, starting to receive packets...");
            let mut buff = alloc::vec![0u8; rx.packet_size() * 10];
            rx.recv(&mut buff).await.unwrap();
            info!("Received {} bytes", buff.len());
        });

        println!("test passed!");
    }

    fn get_igb() -> Option<(Igb, IrqInfo)> {
        let PlatformInfoKind::DeviceTree(fdt) = &global_val().platform_info;
        let fdt = fdt.get();

        let pcie = fdt
            .find_compatible(&["pci-host-ecam-generic"])
            .next()
            .unwrap()
            .into_pci()
            .unwrap();

        let mut pcie_regs = alloc::vec![];

        let mut bar_alloc = SimpleBarAllocator::default();

        for reg in pcie.node.reg().unwrap() {
            println!("pcie reg: {:#x}", reg.address);
            pcie_regs.push(iomap((reg.address as usize).into(), reg.size.unwrap()));
        }

        let base_vaddr = pcie_regs[0];

        for range in pcie.ranges().unwrap() {
            info!("{range:?}");
            match range.space {
                PciSpace::Memory32 => bar_alloc.set_mem32(range.cpu_address as _, range.size as _),
                PciSpace::Memory64 => bar_alloc.set_mem64(range.cpu_address, range.size),
                _ => {}
            }
        }

        let mut root = RootComplexGeneric::new(base_vaddr);

        for header in root.enumerate(None, Some(bar_alloc)) {
            println!("{}", header);
        }

        for header in root.enumerate_keep_bar(None) {
            if let pcie::Header::Endpoint(mut endpoint) = header.header {
                if !Igb::check_vid_did(endpoint.vendor_id, endpoint.device_id) {
                    continue;
                }

                endpoint.update_command(header.root, |cmd| {
                    cmd | CommandRegister::IO_ENABLE
                        | CommandRegister::MEMORY_ENABLE
                        | CommandRegister::BUS_MASTER_ENABLE
                });

                for cap in &mut endpoint.capabilities {
                    match cap {
                        PciCapability::Msi(msi_capability) => {
                            msi_capability.set_enabled(false, &mut *header.root);
                        }
                        PciCapability::MsiX(msix_capability) => {
                            msix_capability.set_enabled(false, &mut *header.root);
                        }
                        _ => {}
                    }
                }

                println!(
                    "irq_pin {:?}, {:?}",
                    endpoint.interrupt_pin, endpoint.interrupt_line
                );

                let bar_addr;
                let bar_size;
                match endpoint.bar {
                    pcie::BarVec::Memory32(bar_vec_t) => {
                        let bar0 = bar_vec_t[0].as_ref().unwrap();
                        bar_addr = bar0.address as usize;
                        bar_size = bar0.size as usize;
                    }
                    pcie::BarVec::Memory64(bar_vec_t) => {
                        let bar0 = bar_vec_t[0].as_ref().unwrap();
                        bar_addr = bar0.address as usize;
                        bar_size = bar0.size as usize;
                    }
                    pcie::BarVec::Io(_bar_vec_t) => todo!(),
                };

                println!("bar0: {:#x}", bar_addr);

                let addr = iomap(bar_addr.into(), bar_size);

                let igb = Igb::new(addr).unwrap();

                let irq = pcie
                    .child_irq_info(
                        endpoint.address.bus(),
                        endpoint.address.device(),
                        endpoint.address.function(),
                        endpoint.interrupt_pin,
                    )
                    .unwrap();
                return Some((igb, irq));
            }
        }
        None
    }
}
struct KernelImpl;

impl_trait! {
    impl Kernel for KernelImpl {
        fn sleep(duration: Duration) {
            spin_delay(duration);
        }
    }
}
