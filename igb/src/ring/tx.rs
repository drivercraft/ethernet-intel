use alloc::sync::Arc;
use log::trace;

use crate::descriptor::{TxAdvDescCmd, TxAdvDescType};

use super::*;

type RingInner = Ring<AdvTxDesc>;

impl RingInner {
    pub fn init(&mut self) -> Result<(), DError> {
        debug!("init tx");
        // Step 1: Allocate a region of memory for the transmit descriptor list
        // (Already done in Ring::new())

        // Step 2: Program the descriptor base address with the address of the region
        self.reg_write(TDBAL, (self.bus_addr() & 0xFFFFFFFF) as u32);
        self.reg_write(TDBAH, (self.bus_addr() >> 32) as u32);

        // Step 3: Set the length register to the size of the descriptor ring
        self.reg_write(TDLEN, self.size_bytes() as u32);

        // Step 4: Program the TXDCTL register with the desired TX descriptor write back policy
        // Suggested values: WTHRESH = 1, all other fields 0
        self.reg_write(TXDCTL, TXDCTL::WTHRESH.val(1).value);

        self.reg_write(TDH, 0);
        self.reg_write(TDT, 0);

        // Step 5: If needed, set the TDWBAL/TWDBAH to enable head write back
        // (Not implemented in this basic version)

        // Step 6: Enable the queue using TXDCTL.ENABLE (queue zero is enabled by default)
        self.reg_write(
            TXDCTL,
            (TXDCTL::WTHRESH.val(1) + TXDCTL::ENABLE::Enabled).value,
        );

        // Step 7: Poll the TXDCTL register until the ENABLE bit is set
        wait_for(
            || self.reg_read(TXDCTL) & TXDCTL::ENABLE::Enabled.value > 0,
            Duration::from_millis(1),
            Some(1000),
        )?;

        // Note: The tail register of the queue (TDT[n]) should not be bumped until the queue is enabled
        // Step 8: Enable transmit path by setting TCTL.EN should be done only after all other settings are done
        // This is handled by the MAC layer through mac.enable_tx()
        debug!("TX ring initialized successfully");
        Ok(())
    }

    /// 获取当前头部指针值
    pub fn get_tx_head(&self) -> u32 {
        self.reg_read(TDH)
    }

    /// 获取当前尾部指针值
    pub fn get_tx_tail(&self) -> u32 {
        self.reg_read(TDT)
    }

    /// 发送单个数据包
    pub fn send_packet(&mut self, buff: &[u8]) -> Result<(), DError> {
        if buff.len() > PACKET_SIZE as usize {
            return Err(DError::InvalidParameter);
        }
        trace!("send {}", buff.len());
        let tail = self.get_tx_tail() as usize;
        let next_tail = (tail + 1) % self.count();
        let head = self.get_tx_head() as usize;

        // 检查是否有空间
        if next_tail == head {
            return Err(DError::NoMemory); // 环形缓冲区已满
        }

        for (i, &v) in buff.iter().enumerate() {
            self.pkts[next_tail].set(i, v);
        }

        let buffer_addr = self.pkts[next_tail].bus_addr();

        // 设置描述符
        let desc = AdvTxDesc::new(
            buffer_addr,
            buff.len(),
            TxAdvDescType::Data,
            &[
                TxAdvDescCmd::EOP,
                TxAdvDescCmd::RS,
                TxAdvDescCmd::IFCS,
                TxAdvDescCmd::DEXT,
            ],
        );

        self.descriptors.set(tail, desc);

        // 内存屏障确保描述符写入完成
        mb();

        // 更新尾部指针
        self.reg_write(TDT, next_tail as u32);

        Ok(())
    }
}

pub struct TxRing(Arc<UnsafeCell<RingInner>>);

unsafe impl Send for TxRing {}

impl TxRing {
    #[allow(clippy::arc_with_non_send_sync)]
    pub(crate) fn new(idx: usize, mmio_base: NonNull<u8>, size: usize) -> Result<Self, DError> {
        let mut ring_inner = Ring::new(
            idx,
            mmio_base,
            size,
            PACKET_SIZE as usize,
            Direction::ToDevice,
        )?;
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

    pub fn send(&mut self, buff: &[u8]) -> Result<(), DError> {
        self.this_mut().send_packet(buff)
    }

    pub fn next_pkt(&mut self) -> Option<TxBuff<'_>> {
        let ring = self.this_mut();
        let next_tail = (ring.get_tx_tail() + 1) % ring.count() as u32;
        if next_tail == ring.get_tx_head() {
            return None; // 没有可用的包
        }

        Some(TxBuff { ring: self })
    }

    pub fn request_max_count(&self) -> usize {
        self.this().count() - 1
    }
}

pub struct TxBuff<'a> {
    ring: &'a mut TxRing,
}

impl TxBuff<'_> {
    pub fn send(self, buff: &[u8]) -> Result<(), DError> {
        if buff.len() > PACKET_SIZE as usize {
            return Err(DError::InvalidParameter);
        }
        self.ring.send(buff)
    }
}
