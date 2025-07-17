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

    // #[test]
    // fn it_works() {
    //     let (igb, irq) = get_igb().unwrap();

    //     info!("igb: {:#?}", igb.status());

    //     let mut igb = Driver::new(igb);
    //     let igb_ptr = igb.0.get();

    //     for one in &irq.cfgs {
    //         IrqParam {
    //             intc: irq.irq_parent,
    //             cfg: one.clone(),
    //         }
    //         .register_builder({
    //             move |_irq| {
    //                 unsafe {
    //                     (*igb_ptr).handle_interrupt();
    //                 }
    //                 IrqHandleResult::Handled
    //             }
    //         })
    //         .register();
    //     }

    //     let mut rx = igb.new_rx_ring().unwrap();
    //     let _tx = igb.new_tx_ring().unwrap();

    //     igb.open().unwrap();
    //     info!("igb opened: {:#?}", igb.status());

    //     info!("mac: {:#?}", igb.read_mac());

    //     info!("waiting for link up...");
    //     while !igb.status().link_up {
    //         spin_delay(Duration::from_secs(1));

    //         info!("status: {:#?}", igb.status());
    //     }

    //     spin_on::spin_on(async move {
    //         info!("link up, starting to receive packets...");
    //         let mut buff = alloc::vec![0u8; rx.packet_size() * 10];
    //         rx.recv(&mut buff).await.unwrap();
    //         info!("Received {} bytes", buff.len());
    //     });

    //     println!("test passed!");
    // }

    #[test]
    fn loopback_test() {
        let (igb, irq) = get_igb().unwrap();

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

        igb.open().unwrap();
        info!("igb opened for loopback test: {:#?}", igb.status());
        // igb.enable_loopback();
        // info!("Loopback mode enabled");
        let mac = igb.read_mac();
        info!("mac: {mac:#?}");

        info!("waiting for link up...");
        while !igb.status().link_up {
            spin_delay(Duration::from_secs(1));
            info!("status: {:#?}", igb.status());
        }

        let mut rx = igb.new_rx_ring().unwrap();
        let mut tx = igb.new_tx_ring().unwrap();
        spin_on::spin_on(async move {
            info!("link up, starting loopback test...");

            // 创建测试数据包
            let test_packet = create_test_packet(&mac.bytes());
            info!("Created test packet with {} bytes", test_packet.len());

            // 发送测试数据包
            info!("Sending test packet...");
            tx.send(&test_packet).await.unwrap();
            info!("Test packet sent");

            // 接收数据包
            let mut rx_buff = alloc::vec![0u8; rx.packet_size() * 2];
            info!("Waiting to receive packet...");
            rx.recv(&mut rx_buff).await.unwrap();
            info!("Received {} bytes", rx_buff.len());

            // 验证收到的数据包
            if verify_loopback_packet(&test_packet, &rx_buff) {
                info!("✓ Loopback test passed! Packet correctly received");
            } else {
                info!("✗ Loopback test failed! Received packet doesn't match sent packet");
            }
        });

        println!("loopback test completed!");
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

    fn create_test_packet(mac_src: &[u8]) -> alloc::vec::Vec<u8> {
        // 创建一个简单的以太网帧用于回环测试
        let mut packet = alloc::vec::Vec::new();

        // 目标MAC地址 (广播地址)
        packet.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

        // 源MAC地址 (测试地址)
        packet.extend_from_slice(mac_src);

        // 以太网类型 (IPv4)
        packet.extend_from_slice(&[0x08, 0x00]);

        // 简单的IPv4头部
        packet.extend_from_slice(&[
            0x45, 0x00, 0x00, 0x2E, // Version, IHL, Type of Service, Total Length
            0x00, 0x00, 0x40, 0x00, // Identification, Flags, Fragment Offset
            0x40, 0x01, 0x00, 0x00, // TTL, Protocol (ICMP), Header Checksum
            0xC0, 0xA8, 0x01, 0x01, // Source IP (192.168.1.1)
            0xC0, 0xA8, 0x01, 0x02, // Destination IP (192.168.1.2)
        ]);

        // ICMP 头部和数据
        packet.extend_from_slice(&[
            0x08, 0x00, 0x00, 0x00, // Type (Echo Request), Code, Checksum
            0x12, 0x34, 0x56, 0x78, // Identifier, Sequence Number
        ]);

        // 测试数据
        packet.extend_from_slice(b"Hello, Loopback Test!");

        // 填充到最小以太网帧大小
        while packet.len() < 60 {
            packet.push(0x00);
        }
        packet
    }

    fn verify_loopback_packet(sent_packet: &[u8], received_buffer: &[u8]) -> bool {
        // 在接收缓冲区中查找发送的数据包
        let packet_size = sent_packet.len();

        // 搜索缓冲区中是否包含我们发送的数据包
        for i in 0..received_buffer.len() {
            if i + packet_size <= received_buffer.len() {
                let chunk = &received_buffer[i..i + packet_size];
                if chunk == sent_packet {
                    info!("Found matching packet at offset {i}");
                    return true;
                }
            }
        }

        // 如果没有找到完全匹配的数据包，至少检查一下是否接收到了数据
        let non_zero_bytes = received_buffer.iter().filter(|&&b| b != 0).count();
        info!(
            "Received {non_zero_bytes} non-zero bytes out of {}",
            received_buffer.len()
        );

        // 检查接收到的数据包的前几个字节
        if received_buffer.len() >= 14 {
            info!("Received packet header: {:02x?}", &received_buffer[0..14]);
        }

        false
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
