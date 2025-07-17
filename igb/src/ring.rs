use core::{cell::UnsafeCell, ops::Deref, pin::Pin, ptr::NonNull, time::Duration};

use alloc::{boxed::Box, vec::Vec};
use dma_api::{DSlice, DSliceMut, DVec, Direction};
use mbarrier::mb;
use tock_registers::register_bitfields;
use futures::{stream::Stream, task::AtomicWaker};

use crate::{
    descriptor::{AdvRxDesc, AdvRxDescRead, Descriptor},
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

const PACKET_SIZE_KB: u32 = 2;
const PACKET_SIZE: u32 = PACKET_SIZE_KB * 1024; 

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

#[derive(Default, Clone)]
struct RingElemMeta{
    // waker: AtomicWaker,
    // is_done: bool,
    buff_ptr: usize,
}

pub struct Ring<D: Descriptor> {
    pub descriptors: DVec<D>,
    ring_base: NonNull<u8>,
    current_head: usize,
    ring_head: usize,
    waker: AtomicWaker,
    meta_ls: Vec<RingElemMeta>,
}

impl<D: Descriptor> Ring<D> {
    pub fn new(idx: usize, mmio_base: NonNull<u8>, size: usize) -> Result<Self, DError> {
        let descriptors =
            DVec::zeros(size, 0x1000, Direction::Bidirectional).ok_or(DError::NoMemory)?;

        let ring_base = unsafe { mmio_base.add(idx * 0x40) };
 
        Ok(Self {
            descriptors,
            ring_base,
            waker: AtomicWaker::new(),
            current_head: 0,
            ring_head: 0,
            meta_ls: alloc::vec![RingElemMeta::default(); size],
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
            (SRRCTL::DESCTYPE::AdvancedOneBuffer 
                // 4kB 包大小
                + SRRCTL::BSIZEPACKET.val(PACKET_SIZE_KB)).value,
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

    /// 检查描述符是否已完成(DD位)
    ///
    /// # 参数
    /// - `desc_index`: 描述符索引
    ///
    /// # 返回
    /// 如果描述符已完成则返回 true，否则返回 false
    pub fn is_descriptor_done(&self, desc_index: usize) -> bool {
        if desc_index >= self.descriptors.len() {
            return false;
        }

        // 检查写回格式中的DD位
        let desc = &self.descriptors[desc_index];
        unsafe {
            let wb = desc.write;
            (wb.lo_dword.fields.hdr_status & crate::descriptor::rx_desc_consts::DD_BIT) != 0
        }
    }

    /// 获取当前头部指针值
    pub fn get_head(&self) -> u32 {
        self.reg_read(RDH)
    }

    /// 获取当前尾部指针值
    pub fn get_tail(&self) -> u32 {
        self.reg_read(RDT)
    }

    /// 更新尾部指针
    pub fn update_tail(&mut self) {
        self.reg_write(RDT, self.current_head as u32);
    }

    pub fn clean(&mut self) {
        // 清理环形缓冲区
        self.ring_head = self.get_head() as usize;
    }

    fn packet_buff(&mut self, index: usize) -> &mut [u8] {
        let ptr = self.meta_ls[index].buff_ptr;
        let buff = unsafe{
            core::slice::from_raw_parts_mut(ptr as *mut u8, PACKET_SIZE as usize)
        };
        {
            let sl = DSliceMut::from(buff, Direction::FromDevice);
            sl.preper_read_all();
        }
        buff
    }

    fn rcv_buff(&mut self, buff: &mut [u8]) -> Result<(), DError> {
        assert!(buff.len() >= PACKET_SIZE as usize, "Buffer too small for packet");
        assert!(buff.len().is_multiple_of(PACKET_SIZE as usize), "Buffer size must be a multiple of packet size");
        let mut bus_addr = {
            let sl = DSliceMut::from(buff, Direction::FromDevice);
            sl.bus_addr()
        };

        let mut i = self.reg_read(RDT);

        let mut buff_left = buff;
        while !buff_left.is_empty() {
            let desc = AdvRxDesc{read: AdvRxDescRead { pkt_addr: bus_addr, hdr_addr: 0 }}; 
            self.descriptors.set(i as usize, desc);
            self.meta_ls[i as usize].buff_ptr = buff_left.as_mut_ptr() as usize;

            i += 1;
            if i >= self.count() as u32{
                i = 0;
            }
            bus_addr += PACKET_SIZE as u64;
            buff_left = &mut buff_left[PACKET_SIZE as usize..];
        }
        mb();
        self.reg_write(RDT, i);

        Ok(())
    }
}

pub struct RxRing(UnsafeCell<Box<Ring<AdvRxDesc>>>);

impl RxRing{
    pub(crate) fn new(ring: Ring<AdvRxDesc>) -> Self {
        Self(UnsafeCell::new( Box::new(ring)))
    }
    
    pub(crate) fn addr(&mut self) -> NonNull<Ring<AdvRxDesc>> {
       unsafe{ NonNull::from( (*self.0.get()).as_mut())}
    }

    pub async fn recv(&mut self, buff: &mut [u8])->Result<(), DError> {
        self.this_mut().rcv_buff(buff)?;
        
        RcvFuture::new(self, buff).await?;
        
            DSliceMut::from(buff, Direction::FromDevice)
                .preper_read_all();
        
        Ok(())
    }

    fn this(&self) -> &Ring<AdvRxDesc> {
        unsafe { &*self.0.get() }
    }
    fn this_mut(&mut self) -> &mut Ring<AdvRxDesc> {
        unsafe { &mut *self.0.get() }
    }

    pub fn packet_size(&self) -> usize {
        PACKET_SIZE as usize
    }
}

pub struct RcvFuture<'a> {
    ring: &'a mut RxRing,
    buffer: &'a mut [u8],
    n: usize,
}

impl<'a> RcvFuture<'a> {
    pub fn new(ring: &'a mut RxRing, buffer: &'a mut [u8]) -> Self {
        Self { ring, buffer, n: 0 }
    }
}

impl <'a> Future for RcvFuture<'a> {
    type Output = Result<(), DError>;

    fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        let this = self.get_mut();
        let ring = unsafe { &mut *this.ring.0.get() };

        while this.n < this.buffer.len() && ring.current_head != ring.ring_head {
            if !ring.is_descriptor_done(ring.current_head){
                break;
            }
            ring.current_head += 1;
            if ring.current_head >= ring.count() {
                ring.current_head = 0;
            }
            this.n += PACKET_SIZE as usize;
        }

        if this.n == this.buffer.len() {
            // 已经接收完所有数据
            return core::task::Poll::Ready(Ok(()));
        }
        
        // 没有可用的描述符，注册唤醒器
        ring.waker.register(cx.waker());
        core::task::Poll::Pending
    }
    
}