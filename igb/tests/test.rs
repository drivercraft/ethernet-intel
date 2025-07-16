#![no_std]
#![no_main]
#![feature(used_with_arg)]

use core::time::Duration;

use bare_test::time::spin_delay;
use eth_igb::{impl_trait, osal::Kernel};

extern crate alloc;
extern crate bare_test;

#[bare_test::tests]
mod tests {
    use core::time::Duration;

    use bare_test::{
        fdt_parser::PciSpace,
        globals::{PlatformInfoKind, global_val},
        mem::iomap,
        println,
        time::spin_delay,
    };
    use eth_igb::Igb;
    use log::info;
    use pcie::{CommandRegister, RootComplexGeneric, SimpleBarAllocator};

    #[test]
    fn it_works() {
        let mut igb = get_igb().unwrap();

        info!("igb: {:#?}", igb.status());
        igb.open().unwrap();
        info!("igb opened: {:#?}", igb.status());

        info!("mac: {:#?}", igb.read_mac());

        info!("waiting for link up...");
        while !igb.status().link_up {
            spin_delay(Duration::from_secs(1));

            info!("status: {:#?}", igb.status());
        }

        println!("test passed!");
    }

    fn get_igb() -> Option<Igb> {
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
            if let pcie::Header::Endpoint(endpoint) = header.header {
                if !Igb::check_vid_did(endpoint.vendor_id, endpoint.device_id) {
                    continue;
                }

                endpoint.update_command(header.root, |cmd| {
                    cmd | CommandRegister::IO_ENABLE
                        | CommandRegister::MEMORY_ENABLE
                        | CommandRegister::BUS_MASTER_ENABLE
                });

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

                let igb = Igb::new(addr);
                return Some(igb);
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
