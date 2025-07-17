pub trait Descriptor {}

// Advanced Receive Descriptor constants
pub mod rx_desc_consts {
    // Read Format bit masks
    pub const ADDR_MASK: u64 = 0xFFFF_FFFF_FFFF_FFFE; // Address bits [63:1]
    pub const NSE_MASK: u64 = 0x0000_0000_0000_0001; // No-Snoop Enable bit [0]
    pub const DD_MASK: u64 = 0x0000_0000_0000_0001; // Descriptor Done bit [0]

    // Write-back Format bit masks and shifts
    // Low DWORD (63:0)
    pub const RSS_HASH_MASK: u32 = 0xFFFF_FFFF; // RSS Hash [31:0]
    pub const FRAG_CSUM_MASK: u32 = 0xFFFF_0000; // Fragment Checksum [31:16]
    pub const FRAG_CSUM_SHIFT: u32 = 16;
    pub const IP_ID_MASK: u32 = 0x0000_FFFF; // IP identification [15:0]

    // 注意：根据Intel文档，位字段应该是：
    // 63:48 - RSS Hash/Fragment Checksum (高16位)
    // 47:32 - IP identification
    // 31:22 - HDR_LEN (Header Length)
    // 21 - SPH (Split Header)
    // 20:0 - Extended Status
    pub const HDR_LEN_MASK: u32 = 0xFFC0_0000; // Header Length [31:22]
    pub const HDR_LEN_SHIFT: u32 = 22;
    pub const SPH_MASK: u32 = 0x0020_0000; // Split Header [21]
    pub const SPH_SHIFT: u32 = 21;
    pub const EXT_STATUS_LO_MASK: u32 = 0x001F_FFFF; // Extended Status [20:0]

    // High DWORD (127:64)
    // 63:48 - VLAN Tag
    // 47:32 - PKT_LEN (Packet Length)
    // 31:20 - Extended Error
    // 19:17 - RSS Type
    // 16:4 - Packet Type
    // 3:0 - Extended Status
    pub const EXT_ERROR_MASK: u32 = 0xFFF0_0000; // Extended Error [31:20]
    pub const EXT_ERROR_SHIFT: u32 = 20;
    pub const RSS_TYPE_MASK: u32 = 0x000E_0000; // RSS Type [19:17]
    pub const RSS_TYPE_SHIFT: u32 = 17;
    pub const PKT_TYPE_MASK: u32 = 0x0001_FFF0; // Packet Type [16:4]
    pub const PKT_TYPE_SHIFT: u32 = 4;
    pub const EXT_STATUS_HI_MASK: u32 = 0x0000_000F; // Extended Status [3:0]

    pub const VLAN_TAG_MASK: u32 = 0xFFFF_0000; // VLAN Tag [31:16]
    pub const VLAN_TAG_SHIFT: u32 = 16;
    pub const PKT_LEN_MASK: u32 = 0x0000_FFFF; // Packet Length [15:0]

    // Extended Status bits
    pub const DD_BIT: u32 = 1 << 0; // Descriptor Done
    pub const EOP_BIT: u32 = 1 << 1; // End of Packet
    pub const VP_BIT: u32 = 1 << 3; // VLAN Packet
    pub const UDPCS_BIT: u32 = 1 << 4; // UDP Checksum
    pub const L4I_BIT: u32 = 1 << 5; // L4 Integrity check
    pub const IPCS_BIT: u32 = 1 << 6; // IP Checksum
    pub const PIF_BIT: u32 = 1 << 7; // Passed In-exact Filter
    pub const VEXT_BIT: u32 = 1 << 9; // First VLAN on double VLAN
    pub const UDPV_BIT: u32 = 1 << 10; // UDP Valid
    pub const LLINT_BIT: u32 = 1 << 11; // Low Latency Interrupt
    pub const SECP_BIT: u32 = 1 << 17; // Security Processing
    pub const LB_BIT: u32 = 1 << 18; // Loopback
    pub const TS_BIT: u32 = 1 << 16; // Time Stamped

    // Extended Error bits
    pub const HBO_BIT: u32 = 1 << 3; // Header Buffer Overflow
    pub const SECERR_MASK: u32 = 0x0000_0180; // Security Error [8:7]
    pub const SECERR_SHIFT: u32 = 7;
    pub const L4E_BIT: u32 = 1 << 9; // L4 Error
    pub const IPE_BIT: u32 = 1 << 10; // IP Error
    pub const RXE_BIT: u32 = 1 << 11; // RX Error
}

// Advanced Transmit Descriptor constants
pub mod tx_desc_consts {
    // CMD_TYPE_LEN field bits
    pub const CMD_EOP: u32 = 1 << 24; // End of Packet
    pub const CMD_IFCS: u32 = 1 << 25; // Insert FCS
    pub const CMD_IC: u32 = 1 << 26; // Insert Checksum
    pub const CMD_RS: u32 = 1 << 27; // Report Status
    pub const CMD_DEXT: u32 = 1 << 29; // Descriptor Extension
    pub const CMD_VLE: u32 = 1 << 30; // VLAN Packet Enable
    pub const CMD_IDE: u32 = 1 << 31; // Interrupt Delay Enable

    // Descriptor types
    pub const DTYPE_DATA: u32 = 0b11 << 20; // Data descriptor
    pub const DTYPE_CONTEXT: u32 = 0b10 << 20; // Context descriptor

    // Length mask
    pub const LEN_MASK: u32 = 0x000F_FFFF; // Packet Length [19:0]

    // Status bits in write-back format
    pub const DD_BIT: u32 = 1 << 0; // Descriptor Done
}

/// RSS类型枚举
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RssType {
    None = 0x0,
    HashTcpIpv4 = 0x1,
    HashIpv4 = 0x2,
    HashTcpIpv6 = 0x3,
    HashIpv6Ex = 0x4,
    HashIpv6 = 0x5,
    HashTcpIpv6Ex = 0x6,
    HashUdpIpv4 = 0x7,
    HashUdpIpv6 = 0x8,
    HashUdpIpv6Ex = 0x9,
    Reserved(u8),
}

impl From<u8> for RssType {
    fn from(val: u8) -> Self {
        match val {
            0x0 => RssType::None,
            0x1 => RssType::HashTcpIpv4,
            0x2 => RssType::HashIpv4,
            0x3 => RssType::HashTcpIpv6,
            0x4 => RssType::HashIpv6Ex,
            0x5 => RssType::HashIpv6,
            0x6 => RssType::HashTcpIpv6Ex,
            0x7 => RssType::HashUdpIpv4,
            0x8 => RssType::HashUdpIpv6,
            0x9 => RssType::HashUdpIpv6Ex,
            _ => RssType::Reserved(val),
        }
    }
}

/// 安全错误类型枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SecurityError {
    None = 0b00,
    NoSaMatch = 0b01,
    ReplayError = 0b10,
    BadSignature = 0b11,
}

impl From<u8> for SecurityError {
    fn from(val: u8) -> Self {
        match val & 0b11 {
            0b00 => SecurityError::None,
            0b01 => SecurityError::NoSaMatch,
            0b10 => SecurityError::ReplayError,
            0b11 => SecurityError::BadSignature,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy)]
pub union AdvTxDesc {
    pub read: AdvTxDescRead,
    pub write: AdvTxDescWB,
}

impl Descriptor for AdvTxDesc {}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct AdvTxDescRead {
    pub buffer_addr: u64,
    pub cmd_type_len: u32,
    pub olinfo_status: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct AdvTxDescWB {
    pub rsvd: u64,
    pub nxtseq_seed: u32,
    pub status: u32,
}

/// Advanced Receive Descriptor (82576EB)
///
/// 这个联合体表示Intel 82576EB控制器的高级接收描述符，
/// 支持软件写入格式（Read Format）和硬件写回格式（Write-back Format）
///
/// 根据Intel 82576EB GbE Controller文档第7.1.5节：
/// - Read Format: 软件写入描述符队列，硬件从主机内存读取
/// - Write-back Format: 硬件写回描述符到主机内存
#[derive(Clone, Copy)]
pub union AdvRxDesc {
    /// 软件写入格式 - 提供包和头部缓冲区地址
    pub read: AdvRxDescRead,
    /// 硬件写回格式 - 包含接收状态、长度、校验和等信息
    pub write: AdvRxDescWB,
}

impl Descriptor for AdvRxDesc {}

/// Advanced Receive Descriptor Read Format (软件写入格式)
///
/// 根据Intel 82576EB文档Table 7-10，这是软件写入描述符队列的格式：
///
/// ```text
/// 63                                                                0
/// +------------------------------------------------------------------+
/// |              Packet Buffer Address [63:1]                | A0/NSE|
/// +------------------------------------------------------------------+
/// |              Header Buffer Address [63:1]                |  DD   |
/// +------------------------------------------------------------------+
/// ```
///
/// - Packet Buffer Address: 包缓冲区的物理地址
/// - A0/NSE: 最低位是地址位A0或No-Snoop Enable位
/// - Header Buffer Address: 头部缓冲区的物理地址  
/// - DD: Descriptor Done位，硬件写回时设置
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AdvRxDescRead {
    /// Packet Buffer Address [63:1] + A0/NSE [0]
    /// 物理地址的最低位是 A0 (地址的LSB) 或 NSE (No-Snoop Enable)
    pub pkt_addr: u64,
    /// Header Buffer Address [63:1] + DD [0]  
    /// 头部缓冲区物理地址，最低位是 DD (Descriptor Done)
    pub hdr_addr: u64,
}
/// Advanced Receive Descriptor Write-back Format (硬件写回格式)
///
/// 根据Intel 82576EB文档Table 7-11，这是硬件写回到主机内存的格式：
///
/// ```text
/// 127                                                               64
/// +------------------------------------------------------------------+
/// |    VLAN Tag   |   PKT_LEN   |Ext Err|RSS|Pkt Type|Ext Status    |
/// +------------------------------------------------------------------+
/// 63            48 47        32 31    21 20 19    17 16         0
/// +------------------------------------------------------------------+
/// |RSS Hash/Frag  |   IP ID     |HDR_LEN|S|  RSV   |Ext Status     |
/// |   Checksum     |             |       |P|        |               |
/// +------------------------------------------------------------------+
/// ```
///
/// 字段说明：
/// - RSS Hash/Fragment Checksum: RSS哈希值或片段校验和
/// - IP ID: IP标识符（用于片段重组）
/// - HDR_LEN: 头部长度
/// - SPH: Split Header位
/// - PKT_LEN: 包长度
/// - VLAN Tag: VLAN标签
/// - Ext Status: 扩展状态（包含DD、EOP等位）
/// - Ext Error: 扩展错误信息
/// - RSS Type: RSS类型
/// - Pkt Type: 包类型
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AdvRxDescWB {
    /// Lower 64 bits of write-back descriptor
    /// 63:48 - RSS Hash Value/Fragment Checksum
    /// 47:32 - IP identification
    /// 31:21 - HDR_LEN (Header Length)
    /// 20 - SPH (Split Header)
    /// 19:0 - Extended Status
    pub lo_dword: LoDword,
    /// Upper 64 bits of write-back descriptor  
    /// 63:48 - VLAN Tag
    /// 47:32 - PKT_LEN (Packet Length)
    /// 31:20 - Extended Error
    /// 19:17 - RSS Type
    /// 16:4 - Packet Type
    /// 3:0 - Extended Status
    pub hi_dword: HiDword,
}

#[derive(Clone, Copy)]
pub union LoDword {
    /// 完整的64位数据
    pub data: u64,
    /// RSS Hash / Fragment Checksum + IP identification + HDR_LEN + SPH + Extended Status
    pub fields: LoFields,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct LoFields {
    /// 31:0 - RSS Hash Value (32-bit) 或 Fragment Checksum (16-bit, 31:16) + IP identification (16-bit, 15:0)
    pub rss_hash_or_csum_ip: u32,
    /// 63:32 - HDR_LEN (31:22) + SPH (21) + RSV (20:17) + Extended Status (16:0)
    pub hdr_status: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub union HiDword {
    /// 完整的64位数据
    pub data: u64,
    /// 分解的字段
    pub fields: HiFields,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct HiFields {
    /// 31:0 - Extended Error (31:20) + RSS Type (19:17) + Packet Type (16:4) + Extended Status (3:0)
    pub error_type_status: u32,
    /// 63:32 - VLAN Tag (31:16) + PKT_LEN (15:0)
    pub vlan_length: u32,
}

impl AdvRxDescRead {
    /// 创建新的接收描述符
    pub fn new(pkt_addr: u64, hdr_addr: u64, nse: bool) -> Self {
        let pkt_addr = if nse {
            pkt_addr | rx_desc_consts::NSE_MASK
        } else {
            pkt_addr & !rx_desc_consts::NSE_MASK
        };

        Self {
            pkt_addr,
            hdr_addr: hdr_addr & !rx_desc_consts::DD_MASK, // 确保DD位为0
        }
    }

    /// 获取包缓冲区地址
    pub fn packet_buffer_addr(&self) -> u64 {
        self.pkt_addr & rx_desc_consts::ADDR_MASK
    }

    /// 获取头部缓冲区地址
    pub fn header_buffer_addr(&self) -> u64 {
        self.hdr_addr & rx_desc_consts::ADDR_MASK
    }

    /// 检查是否启用了No-Snoop
    pub fn no_snoop_enabled(&self) -> bool {
        self.pkt_addr & rx_desc_consts::NSE_MASK != 0
    }
}

impl AdvRxDescWB {
    /// 检查描述符是否已完成 (DD bit)
    pub fn is_done(&self) -> bool {
        unsafe { self.hi_dword.fields.error_type_status & rx_desc_consts::DD_BIT != 0 }
    }

    /// 检查是否为包的最后一个描述符 (EOP bit)
    pub fn is_end_of_packet(&self) -> bool {
        unsafe { self.hi_dword.fields.error_type_status & rx_desc_consts::EOP_BIT != 0 }
    }

    /// 获取包长度
    pub fn packet_length(&self) -> u16 {
        unsafe { (self.hi_dword.fields.vlan_length & rx_desc_consts::PKT_LEN_MASK) as u16 }
    }

    /// 获取VLAN标签
    pub fn vlan_tag(&self) -> u16 {
        unsafe {
            ((self.hi_dword.fields.vlan_length & rx_desc_consts::VLAN_TAG_MASK)
                >> rx_desc_consts::VLAN_TAG_SHIFT) as u16
        }
    }

    /// 获取RSS哈希值
    pub fn rss_hash(&self) -> u32 {
        unsafe { self.lo_dword.fields.rss_hash_or_csum_ip }
    }

    /// 获取头部长度
    pub fn header_length(&self) -> u16 {
        unsafe {
            ((self.lo_dword.fields.hdr_status & rx_desc_consts::HDR_LEN_MASK)
                >> rx_desc_consts::HDR_LEN_SHIFT) as u16
        }
    }

    /// 检查是否分割头部 (SPH bit)
    pub fn is_split_header(&self) -> bool {
        unsafe { self.lo_dword.fields.hdr_status & rx_desc_consts::SPH_MASK != 0 }
    }

    /// 获取包类型
    pub fn packet_type(&self) -> u16 {
        unsafe {
            ((self.hi_dword.fields.error_type_status & rx_desc_consts::PKT_TYPE_MASK)
                >> rx_desc_consts::PKT_TYPE_SHIFT) as u16
        }
    }

    /// 获取RSS类型
    pub fn rss_type(&self) -> u8 {
        unsafe {
            ((self.hi_dword.fields.error_type_status & rx_desc_consts::RSS_TYPE_MASK)
                >> rx_desc_consts::RSS_TYPE_SHIFT) as u8
        }
    }

    /// 检查是否有错误
    pub fn has_errors(&self) -> bool {
        unsafe {
            (self.hi_dword.fields.error_type_status & rx_desc_consts::EXT_ERROR_MASK) != 0
                || (self.hi_dword.fields.error_type_status
                    & (rx_desc_consts::L4E_BIT | rx_desc_consts::IPE_BIT | rx_desc_consts::RXE_BIT))
                    != 0
        }
    }

    /// 检查IP校验和是否有效
    pub fn ip_checksum_valid(&self) -> bool {
        unsafe {
            (self.hi_dword.fields.error_type_status & rx_desc_consts::IPCS_BIT) != 0
                && (self.hi_dword.fields.error_type_status & rx_desc_consts::IPE_BIT) == 0
        }
    }

    /// 检查L4校验和是否有效
    pub fn l4_checksum_valid(&self) -> bool {
        unsafe {
            (self.hi_dword.fields.error_type_status & rx_desc_consts::L4I_BIT) != 0
                && (self.hi_dword.fields.error_type_status & rx_desc_consts::L4E_BIT) == 0
        }
    }

    /// 获取RSS类型枚举
    pub fn rss_type_enum(&self) -> RssType {
        RssType::from(self.rss_type())
    }

    /// 获取安全错误类型
    pub fn security_error(&self) -> SecurityError {
        unsafe {
            let error_bits = (self.hi_dword.fields.error_type_status & rx_desc_consts::SECERR_MASK)
                >> rx_desc_consts::SECERR_SHIFT;
            SecurityError::from(error_bits as u8)
        }
    }

    /// 检查是否有头部缓冲区溢出
    pub fn has_header_buffer_overflow(&self) -> bool {
        unsafe { (self.hi_dword.fields.error_type_status & rx_desc_consts::HBO_BIT) != 0 }
    }

    /// 检查是否为VLAN包
    pub fn is_vlan_packet(&self) -> bool {
        unsafe { (self.hi_dword.fields.error_type_status & rx_desc_consts::VP_BIT) != 0 }
    }

    /// 检查是否为回环包
    pub fn is_loopback_packet(&self) -> bool {
        unsafe { (self.hi_dword.fields.error_type_status & rx_desc_consts::LB_BIT) != 0 }
    }

    /// 检查是否为时间戳包
    pub fn is_timestamped(&self) -> bool {
        unsafe { (self.hi_dword.fields.error_type_status & rx_desc_consts::TS_BIT) != 0 }
    }

    /// 获取片段校验和（当不使用RSS时）
    pub fn fragment_checksum(&self) -> u16 {
        unsafe {
            ((self.lo_dword.fields.rss_hash_or_csum_ip & rx_desc_consts::FRAG_CSUM_MASK)
                >> rx_desc_consts::FRAG_CSUM_SHIFT) as u16
        }
    }

    /// 获取IP标识符（当不使用RSS时）
    pub fn ip_identification(&self) -> u16 {
        unsafe { (self.lo_dword.fields.rss_hash_or_csum_ip & rx_desc_consts::IP_ID_MASK) as u16 }
    }
}
