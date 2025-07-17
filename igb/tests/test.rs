#![no_std]
#![no_main]
#![feature(used_with_arg)]

use core::future::Future;
use core::time::Duration;

use bare_test::time::spin_delay;
use eth_igb::{impl_trait, osal::Kernel};

extern crate alloc;

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
    use eth_igb::{Igb, RxBuff};
    use log::{debug, info};
    use pcie::{CommandRegister, PciCapability, RootComplexGeneric, SimpleBarAllocator};
    use smoltcp::socket::icmp::{self, Socket as IcmpSocket};
    use smoltcp::storage::PacketMetadata;
    use smoltcp::time::Instant;
    use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
    use smoltcp::{
        iface::{Config, Interface, SocketSet},
        wire::HardwareAddress,
    };
    use smoltcp::{
        phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
        wire::{Icmpv4Packet, Icmpv4Repr},
    };

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
    unsafe impl<T> Send for Driver<T> {}
    unsafe impl<T> Sync for Driver<T> {}

    impl<T> Driver<T> {
        pub fn new(inner: T) -> Self {
            Self(UnsafeCell::new(inner))
        }
    }

    // SmolTCP device adapter for IGB
    struct IgbDevice {
        rx_ring: eth_igb::RxRing,
        tx_ring: eth_igb::TxRing,
    }

    impl IgbDevice {
        fn new(rx_ring: eth_igb::RxRing, tx_ring: eth_igb::TxRing) -> Self {
            Self { rx_ring, tx_ring }
        }
    }

    impl Device for IgbDevice {
        type RxToken<'a> = IgbRxToken<'a>;
        type TxToken<'a> = IgbTxToken<'a>;

        fn receive(
            &mut self,
            _timestamp: Instant,
        ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
            self.rx_ring.next_pkt().map(|buff| {
                let rx_token = IgbRxToken { buff };
                let tx_token = IgbTxToken {
                    tx_ring: &mut self.tx_ring,
                };
                (rx_token, tx_token)
            })
        }

        fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
            Some(IgbTxToken {
                tx_ring: &mut self.tx_ring,
            })
        }

        fn capabilities(&self) -> DeviceCapabilities {
            let mut caps = DeviceCapabilities::default();
            caps.max_transmission_unit = 1500;
            caps.max_burst_size = Some(1);
            caps.medium = Medium::Ethernet;
            caps
        }
    }

    struct IgbRxToken<'a> {
        buff: RxBuff<'a>,
    }

    impl<'a> RxToken for IgbRxToken<'a> {
        fn consume<R, F>(self, f: F) -> R
        where
            F: FnOnce(&[u8]) -> R,
        {
            f(&self.buff)
        }
    }

    struct IgbTxToken<'a> {
        tx_ring: &'a mut eth_igb::TxRing,
    }

    impl<'a> TxToken for IgbTxToken<'a> {
        fn consume<R, F>(self, len: usize, f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            let mut buffer = alloc::vec![0u8; len];
            let result = f(&mut buffer);
            // 异步发送数据包
            self.tx_ring.send(&buffer).unwrap();

            result
        }
    }

    fn now() -> Instant {
        let ms = bare_test::time::since_boot().as_millis() as u64;
        Instant::from_millis(ms as i64)
    }

    #[test]
    fn ping_test() {
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
        info!("igb opened for ping test: {:#?}", igb.status());

        let mac = igb.read_mac();
        info!("mac: {mac:#?}");

        info!("waiting for link up...");
        while !igb.status().link_up {
            spin_delay(Duration::from_secs(1));
            info!("status: {:#?}", igb.status());
        }

        let (tx_ring, rx_ring) = igb.new_ring().unwrap();

        // 创建 smoltcp 设备适配器
        let mut device = IgbDevice::new(rx_ring, tx_ring);

        // 设置网络配置
        let config = Config::new(HardwareAddress::Ethernet(EthernetAddress::from_bytes(
            &mac.bytes(),
        )));
        let mut iface = Interface::new(config, &mut device, now());

        // 配置 IP 地址
        let ip_addr = IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8);
        iface.update_ip_addrs(|ip_addrs| {
            ip_addrs.push(ip_addr).unwrap();
        });

        // 创建 ICMP socket
        let icmp_rx_buffer = icmp::PacketBuffer::new(
            alloc::vec![icmp::PacketMetadata::EMPTY],
            alloc::vec![0; 256],
        );
        let icmp_tx_buffer = icmp::PacketBuffer::new(
            alloc::vec![icmp::PacketMetadata::EMPTY],
            alloc::vec![0; 256],
        );

        let icmp_socket = icmp::Socket::new(icmp_rx_buffer, icmp_tx_buffer);

        let mut socket_set = SocketSet::new(alloc::vec![]);
        let icmp_handle = socket_set.add(icmp_socket);

        info!("Starting ping to 127.0.0.1...");

        // 执行 ping 测试
        let ping_result = ping_127_0_0_1(&mut iface, &mut device, &mut socket_set, icmp_handle);

        if ping_result {
            info!("✓ Ping test passed! Successfully pinged 127.0.0.1");
        } else {
            info!("✗ Ping test failed!");
        }

        println!("ping test completed!");
    }

    fn ping_127_0_0_1(
        iface: &mut Interface,
        device: &mut IgbDevice,
        socket_set: &mut SocketSet,
        icmp_handle: smoltcp::iface::SocketHandle,
    ) -> bool {
        let target_addr = Ipv4Address::new(127, 0, 0, 1);
        let mut ping_sent = false;
        let mut ping_received = false;
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 10;
        let ident = 0x22b;

        while attempts < MAX_ATTEMPTS && !ping_received {
            iface.poll(now(), device, socket_set);
            // 获取 ICMP socket
            let socket = socket_set.get_mut::<IcmpSocket>(icmp_handle);

            if !socket.is_open() {
                socket.bind(icmp::Endpoint::Ident(ident)).unwrap();
            }

            if !ping_sent && socket.can_send() {
                let icmp_repr = Icmpv4Repr::EchoRequest {
                    ident,
                    seq_no: attempts as u16,
                    data: b"ping test",
                };
                let icmp_payload = socket
                    .send(icmp_repr.buffer_len(), target_addr.into())
                    .unwrap();
                let mut icmp_packet = Icmpv4Packet::new_unchecked(icmp_payload);

                // 发送 ping
                icmp_repr.emit(&mut icmp_packet, &device.capabilities().checksum);
            }

            if ping_sent && socket.can_recv() {
                // 接收 ping 响应
                match socket.recv() {
                    Ok((data, addr)) => {
                        info!(
                            "Ping response received from {:?}: {:?}",
                            addr,
                            core::str::from_utf8(data)
                        );
                        ping_received = true;
                    }
                    Err(e) => {
                        info!("Failed to receive ping response: {e:?}");
                    }
                }
            }

            attempts += 1;
            spin_delay(Duration::from_millis(100));
        }

        ping_received
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
