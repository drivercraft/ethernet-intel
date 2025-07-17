use core::ops::{Deref, DerefMut};

use alloc::{sync::Arc, vec::Vec};
use dma_api::DVec;
use super::*;
use crate::{descriptor::AdvRxDesc, DError};

struct RingInner {
    base: Ring<AdvRxDesc>,
    pkts: Vec<DVec<u8>>,
}

impl RingInner {
    fn new(ring: Ring<AdvRxDesc>, pkt_size: usize) -> Result< Self, DError> {
        let mut pkts = Vec::with_capacity(ring.count());
        for _ in 0..ring.count() {
            pkts.push(DVec::zeros(pkt_size, pkt_size, Direction::FromDevice).ok_or(DError::NoMemory)?);
        }
        Ok( Self { base:ring, pkts })
    }

    fn init(&mut self) -> Result<(), DError> {
        let bus_addr = self.bus_addr();
        let size_bytes = self.size_bytes();

        for i in 0..self.descriptors.len() {
            let pkt_addr = self.pkts[i].bus_addr();
            let desc = AdvRxDesc{read: AdvRxDescRead { pkt_addr, hdr_addr: 0 }};
            self.descriptors.set(i, desc);
        }

        // Program the descriptor base address with the address of the region.
        self.reg_write(RDBAL, (bus_addr & 0xFFFFFFFF) as u32);
        self.reg_write(RDBAH, (bus_addr >> 32) as u32);

        // Set the length register to the size of the descriptor ring.
        self.reg_write(RDLEN, size_bytes as u32);

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
            (wb.hi_dword.fields.error_type_status & crate::descriptor::rx_desc_consts::DD_BIT) != 0
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
        let tail = self.current_head;
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

impl RxRing{
    pub(crate) fn new(idx: usize, mmio_base: NonNull<u8>, size: usize) ->Result<Self, DError>  {
        let base = Ring::new(idx, mmio_base, size)?;
        let ring_inner = RingInner::new(base, PACKET_SIZE as usize)?;
        let ring = Arc::new(UnsafeCell::new(ring_inner));
        Ok(Self(ring))
    }
    
    pub(crate) fn addr(&mut self) -> NonNull<Ring<AdvRxDesc>> {
       unsafe{ NonNull::from( (*self.0.get()).as_mut())}
    }

    pub async fn recv(&mut self, buff: &mut [u8])->Result<(), DError> {
        self.this_mut().rcv_buff(buff)?;
        
        RcvFuture::new(self).await?;
        
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
}

impl<'a> RcvFuture<'a> {
    pub fn new(ring: &'a mut RxRing) -> Self {
        Self { ring }
    }
}

impl <'a> Future for RcvFuture<'a> {
    type Output = Result<(), DError>;

    fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        let this = self.get_mut();
        let ring = unsafe { &mut *this.ring.0.get() };

        while this.n < this.buffer.len() && ring.current_head != ring.hw_head {
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

