use core::{ptr::NonNull, time::Duration};

use dma_api::{DVec, Direction};
use tock_registers::register_bitfields;

use crate::{
    descriptor::{AdvRxDesc, Descriptor},
    err::DError,
    osal::wait_for,
};

pub const DEFAULT_RING_SIZE: usize = 256;
const RDBAL: usize = 0xC000; // RX Descriptor Base Address Low
const RDBAH: usize = 0xC004; // RX Descriptor Base Address High
const RDLEN: usize = 0xC008; // RX Descriptor Length
const SRRCTL: usize = 0xC00C; // RX Descriptor Control
const RDH: usize = 0xC010; // RX Descriptor Head
const RDT: usize = 0xC018; // RX Descriptor Tail
const RXDCTL: usize = 0xC028; // RX Descriptor Control
// const RXCTL: usize = 0xC014; // RX Control
// const RQDPC: usize = 0xC030; // RX Descriptor Polling Control

register_bitfields! [
    // First parameter is the register width. Can be u8, u16, u32, or u64.
    u32,

    RDLEN [
        LEN OFFSET(7) NUMBITS(13)[],
    ],

    pub SRRCTL [
        BSIZEPACKET OFFSET(0) NUMBITS(7)[],
        BSIZEHEADER OFFSET(8) NUMBITS(4)[],
        RDMTS OFFSET(20) NUMBITS(5)[],
        DESCTYPE OFFSET(25) NUMBITS(3)[
            Legacy = 0b000,
            AdvancedOneBuffer = 0b001,
            AdvancedHeaderSplitting = 0b010,
            AdvancedHeaderReplicationAlways = 0b011,
            AdvancedHeaderReplicationLargePacket = 0b100,
        ],
        SECRC OFFSET(26) NUMBITS(1)[
            DoNotStrip = 0,
            Strip = 1,
        ],
        DROP_EN OFFSET(31) NUMBITS(1)[
            Disabled = 0,
            Enabled = 1,
        ],
    ],

    pub RXDCTL [
        PTHRESH OFFSET(0) NUMBITS(5)[],
        HTHRESH OFFSET(8) NUMBITS(5)[],
        WTHRESH OFFSET(16) NUMBITS(5)[],
        ENABLE OFFSET(25) NUMBITS(1)[
            Disabled = 0,
            Enabled = 1,
        ],
        SWFLUSH OFFSET(26) NUMBITS(1)[],
    ]
];

pub struct Ring<D: Descriptor> {
    pub descriptors: DVec<D>,
    ring_base: NonNull<u8>,
}

impl<D: Descriptor> Ring<D> {
    pub fn new(idx: usize, mmio_base: NonNull<u8>, size: usize) -> Result<Self, DError> {
        let descriptors =
            DVec::zeros(size, 0x1000, Direction::Bidirectional).ok_or(DError::NoMemory)?;

        let ring_base = unsafe { mmio_base.add(idx * 0x40) };

        Ok(Self {
            descriptors,
            ring_base,
        })
    }

    pub fn bus_addr(&self) -> u64 {
        // 获取 DMA 物理地址
        // 暂时返回虚拟地址，这里需要根据实际的 DMA API 实现
        self.descriptors.bus_addr()
    }

    pub fn size_bytes(&self) -> usize {
        self.descriptors.len() * core::mem::size_of::<D>()
    }

    pub fn count(&self) -> usize {
        self.descriptors.len()
    }

    fn reg_addr(&self, reg: usize) -> NonNull<u32> {
        unsafe { self.ring_base.add(reg).cast() }
    }

    fn reg_write(&mut self, reg: usize, value: u32) {
        unsafe {
            self.reg_addr(reg).write_volatile(value);
        }
    }
    fn reg_read(&self, reg: usize) -> u32 {
        unsafe { self.reg_addr(reg).read_volatile() }
    }
}

impl Ring<AdvRxDesc> {
    pub fn init(&mut self) -> Result<(), DError> {
        // Program the descriptor base address with the address of the region.
        self.reg_write(RDBAL, (self.bus_addr() & 0xFFFFFFFF) as u32);
        self.reg_write(RDBAH, (self.bus_addr() >> 32) as u32);

        // Set the length register to the size of the descriptor ring.
        self.reg_write(RDLEN, self.size_bytes() as u32);

        // Program SRRCTL of the queue according to the size of the buffers and the required header handling.
        self.reg_write(
            SRRCTL,
            (SRRCTL::DESCTYPE::AdvancedOneBuffer + SRRCTL::BSIZEPACKET.val(2)).value,
        );

        // If header split or header replication is required for this queue,
        // program the PSRTYPE register according to the required headers.
        // 暂时不需要头部分割

        self.reg_write(RDH, 0);
        self.reg_write(RDT, 0);

        // Enable the queue by setting RXDCTL.ENABLE. In the case of queue zero,
        // the enable bit is set by default - so the ring parameters should be set before RCTL.RXEN is set.
        // 使用推荐的阈值：PTHRESH=8, HTHRESH=8, WTHRESH=1
        self.enable_queue();

        // Poll the RXDCTL register until the ENABLE bit is set.
        // The tail should not be bumped before this bit was read as one.

        wait_for(
            || self.reg_read(RXDCTL) & RXDCTL::ENABLE::Enabled.value > 0,
            Duration::from_millis(1),
            Some(1000),
        )?;

        // Program the direction of packets to this queue according to the mode select in MRQC.
        // Packets directed to a disabled queue is dropped.
        // 暂时不配置 MRQC

        // Note: The tail register of the queue (RDT[n]) should not be bumped until the queue is enabled.
        // 队列启用后，更新尾指针
        let ring_count = self.count() as u32;
        self.reg_write(RDT, ring_count - 1);

        Ok(())
    }

    pub fn enable_queue(&mut self) {
        // 启用队列
        self.reg_write(
            RXDCTL,
            (RXDCTL::PTHRESH.val(8)
                + RXDCTL::HTHRESH.val(8)
                + RXDCTL::WTHRESH.val(1)
                + RXDCTL::ENABLE::Enabled)
                .value,
        );
    }

    pub fn disable_queue(&mut self) {
        // 禁用队列
        self.reg_write(
            RXDCTL,
            (RXDCTL::PTHRESH.val(8)
                + RXDCTL::HTHRESH.val(8)
                + RXDCTL::WTHRESH.val(1)
                + RXDCTL::ENABLE::Disabled)
                .value,
        );
    }

    pub fn flush_descriptors(&mut self) {
        // 触发描述符写回刷新
        self.reg_write(
            RXDCTL,
            (RXDCTL::PTHRESH.val(8)
                + RXDCTL::HTHRESH.val(8)
                + RXDCTL::WTHRESH.val(1)
                + RXDCTL::ENABLE::Enabled
                + RXDCTL::SWFLUSH.val(1))
            .value,
        );
    }
}
