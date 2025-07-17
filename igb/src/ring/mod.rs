use core::{cell::UnsafeCell, pin::Pin, ptr::NonNull, time::Duration};

use alloc::{boxed::Box, vec::Vec};
use dma_api::{DSlice, DSliceMut, DVec, Direction};
use log::debug;
use mbarrier::mb;
use tock_registers::register_bitfields;
use futures::task::AtomicWaker;

use crate::{
    descriptor::{AdvRxDesc, AdvRxDescRead, AdvTxDesc, Descriptor},
    err::DError,
    osal::wait_for,
};

mod rx;

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

// TX descriptor registers
const TDBAL: usize = 0xE000; // TX Descriptor Base Address Low
const TDBAH: usize = 0xE004; // TX Descriptor Base Address High
const TDLEN: usize = 0xE008; // TX Descriptor Length
const TDH: usize = 0xE010; // TX Descriptor Head
const TDT: usize = 0xE018; // TX Descriptor Tail
const TXDCTL: usize = 0xE028; // TX Descriptor Control
const TDWBAL: usize = 0xE038; // TX Descriptor Write Back Address Low
const TWDBAH: usize = 0xE03C; // TX Descriptor Write Back Address High

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
    ],

    pub TXDCTL [
        PTHRESH OFFSET(0) NUMBITS(5)[],
        HTHRESH OFFSET(8) NUMBITS(5)[],
        WTHRESH OFFSET(16) NUMBITS(5)[],
        ENABLE OFFSET(25) NUMBITS(1)[
            Disabled = 0,
            Enabled = 1,
        ],
        SWFLUSH OFFSET(26) NUMBITS(1)[],
    ],


];

#[derive(Default, Clone)]
struct RingElemMeta{
    buff_ptr: usize,
}

struct Ring<D: Descriptor> {
    pub descriptors: DVec<D>,
    ring_base: NonNull<u8>,
    current_head: usize,
    hw_head: usize,
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
            hw_head: 0,
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

        self.buffer.set(buff);
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


impl Ring<AdvTxDesc> {
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
        self.reg_write(
            TXDCTL,
            TXDCTL::WTHRESH.val(1).value,
        );

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

    /// 检查描述符是否已完成(DD位)
    pub fn is_tx_descriptor_done(&self, desc_index: usize) -> bool {
        if desc_index >= self.descriptors.len() {
            return false;
        }

        // 检查写回格式中的DD位
        let desc = &self.descriptors[desc_index];
        unsafe {
            let wb = desc.write;
            (wb.status & crate::descriptor::tx_desc_consts::DD_BIT) != 0
        }
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
        
        let tail = self.get_tx_tail() as usize;
        let next_tail = (tail + 1) % self.count();
        let head = self.get_tx_head() as usize;

        // 检查是否有空间
        if next_tail == head {
            return Err(DError::NoMemory); // 环形缓冲区已满
        }

        // 准备DMA缓冲区
        let dma_buff = {
            let sl = DSlice::from(buff);
            sl.bus_addr()
        };

        // 设置描述符
        let desc = AdvTxDesc {
            read: crate::descriptor::AdvTxDescRead {
                buffer_addr: dma_buff,
                cmd_type_len: crate::descriptor::tx_desc_consts::CMD_EOP
                    | crate::descriptor::tx_desc_consts::CMD_IFCS
                    | crate::descriptor::tx_desc_consts::CMD_RS
                    | crate::descriptor::tx_desc_consts::CMD_DEXT
                    | crate::descriptor::tx_desc_consts::DTYPE_DATA
                    | (buff.len() as u32 & crate::descriptor::tx_desc_consts::LEN_MASK),
                olinfo_status: 0,
            },
        };

        self.descriptors.set(tail, desc);
        
        // 内存屏障确保描述符写入完成
        mb();

        // 更新尾部指针
        self.reg_write(TDT, next_tail as u32);

        Ok(())
    }
}

pub struct TxRing(UnsafeCell<Box<Ring<AdvTxDesc>>>);

impl TxRing {
    pub(crate) fn new(ring: Ring<AdvTxDesc>) -> Self {
        Self(UnsafeCell::new(Box::new(ring)))
    }

    pub(crate) fn addr(&mut self) -> NonNull<Ring<AdvTxDesc>> {
        unsafe { NonNull::from((*self.0.get()).as_mut()) }
    }

    pub fn this(&self) -> &Ring<AdvTxDesc> {
        unsafe { &*self.0.get() }
    }

    pub fn this_mut(&mut self) -> &mut Ring<AdvTxDesc> {
        unsafe { &mut *self.0.get() }
    }
    
    pub async fn send(&mut self, buff: &[u8]) -> Result<(), DError> {
        debug!("tx send {}", buff.len());
        for chuck in buff.chunks(PACKET_SIZE as usize) {
            self.this_mut().send_packet(chuck)?;
        }
        Ok(())
    }

}


#[derive(Default)]
struct Buffer{
    ptr: usize,
    len: usize,
    n: usize,
}

impl Buffer {
    fn set(&mut self, buff: &[u8]) {
        self.ptr = buff.as_ptr() as usize;
        self.len = buff.len();
        self.n = 0;
    }
}