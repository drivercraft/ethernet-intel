use core::ops::{Deref, DerefMut};

use super::*;
use crate::{
    DError,
    descriptor::{AdvRxDesc, AdvRxDescRead},
};
use alloc::sync::Arc;
use log::trace;

struct RingInner {
    base: Ring<AdvRxDesc>,
}

impl RingInner {
    fn new(ring: Ring<AdvRxDesc>) -> Result<Self, DError> {
        Ok(Self { base: ring })
    }

    fn init(&mut self) -> Result<(), DError> {
        let bus_addr = self.bus_addr();
        let size_bytes = self.size_bytes();

        for i in 0..self.descriptors.len() {
            let pkt_addr = self.pkts[i].bus_addr();
            let desc = AdvRxDesc {
                read: AdvRxDescRead {
                    pkt_addr,
                    hdr_addr: 0,
                },
            };
            self.descriptors.set(i, desc);
        }

        // Program the descriptor base address with the address of the region.
        self.reg_write(RDBAL, (bus_addr & 0xFFFFFFFF) as u32);
        self.reg_write(RDBAH, (bus_addr >> 32) as u32);

        // Set the length register to the size of the descriptor ring.
        self.reg_write(RDLEN, size_bytes as u32);

        let pkt_size_kb = self.pkt_size / 1024;

        // Program SRRCTL of the queue according to the size of the buffers and the required header handling.
        self.reg_write(
            SRRCTL,
            (SRRCTL::DESCTYPE::AdvancedOneBuffer + SRRCTL::BSIZEPACKET.val(pkt_size_kb as _)).value,
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
        self.update_tail(self.descriptors.len() - 1);
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
            wb.is_done()
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
    pub fn update_tail(&mut self, mut tail: usize) {
        if tail == self.descriptors.len() {
            tail = 0;
        }
        self.reg_write(RDT, tail as u32);
    }

    pub fn clean(&mut self) {
        // 清理环形缓冲区
        self.hw_head = self.get_head() as usize;
    }
}
impl Deref for RingInner {
    type Target = super::Ring<AdvRxDesc>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for RingInner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

pub struct RxRing(Arc<UnsafeCell<RingInner>>);

unsafe impl Send for RxRing {}

impl RxRing {
    pub(crate) fn new(idx: usize, mmio_base: NonNull<u8>, size: usize) -> Result<Self, DError> {
        let base = Ring::new(
            idx,
            mmio_base,
            size,
            PACKET_SIZE as usize,
            Direction::FromDevice,
        )?;
        let mut ring_inner = RingInner::new(base)?;
        ring_inner.init()?;
        let ring = Arc::new(UnsafeCell::new(ring_inner));
        Ok(Self(ring))
    }

    fn this(&self) -> &RingInner {
        unsafe { &*self.0.get() }
    }
    fn this_mut(&mut self) -> &mut RingInner {
        unsafe { &mut *self.0.get() }
    }

    pub fn packet_size(&self) -> usize {
        self.this().pkt_size
    }

    pub fn next_pkt(&mut self) -> Option<RxBuff<'_>> {
        let ring = self.this_mut();
        let head = ring.get_head() as usize;
        let tail = ring.get_tail() as usize;
        trace!("RxRing: next_pkt head: {head}, tail: {tail}");
        let index = (tail + 1) % ring.count();
        if head == index {
            return None; // 没有可用的缓冲区
        }

        // 检查描述符是否已完成
        if !ring.is_descriptor_done(index) {
            trace!("RxRing: next_pkt descriptor not done at index: {index}");
            return None; // 描述符未完成，无法获取数据
        }
        trace!("RxRing: next_pkt index: {index}");
        // 返回 RxBuff 实例
        Some(RxBuff { ring: self, index })
    }
}

impl Drop for RxRing {
    fn drop(&mut self) {
        // 在释放时禁用队列
        self.this_mut().disable_queue();
    }
}

pub struct RxBuff<'a> {
    ring: &'a mut RxRing,
    index: usize,
}

impl Deref for RxBuff<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.ring.this().pkts[self.index].deref()
    }
}

impl Drop for RxBuff<'_> {
    fn drop(&mut self) {
        // 在释放时更新尾部指针
        self.ring.this_mut().update_tail(self.index);
    }
}
