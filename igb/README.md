# Intel IGB Ethernet Driver

A Rust-based Intel IGB Ethernet driver supporting 82576 series network controllers.

## Features

- **Hardware Support**: Supports Intel 82576 series Ethernet controllers
- **Ring Buffers**: Efficient transmit and receive ring buffer implementation
- **Zero-Copy**: DMA-based zero-copy data transfer

## Supported Devices

- **Vendor ID**: 0x8086 (Intel)
- **Device ID**:
  - 0x10C9 (82576 Gigabit Network Connection)
  - 0x1533 (I210 Gigabit Network Connection)

## Usage Examples

### Basic Initialization

First impl [dma-api](https://crates.io/crates/dma-api)

```rust
use eth_igb::{Igb, Request};

struct KernelImpl;

eth_igb::impl_trait! {
    impl Kernel for KernelImpl {
        fn sleep(duration: Duration) {
            your_os::spin_delay(duration);
        }
    }
}


// Create driver instance
let mut igb = Igb::new(iobase)?;

// Open device
igb.open()?;

// Create transmit and receive rings
let (tx_ring, rx_ring) = igb.new_ring()?;
```

### Sending Packets

```rust
// Prepare transmission data
let data = vec![0u8; 1500];
let request = Request::new_tx(data);

// Send packet
tx_ring.send(request)?;
```

### Receiving Packets

```rust
// Prepare receive buffer
let buff = vec![0u8; rx_ring.packet_size()];
let request = Request::new_rx(buff);
rx_ring.submit(request)?;

// Receive packet
if let Some(packet) = rx_ring.next_pkt() {
    println!("Received packet: {} bytes", packet.len());
}
```

## Testing

The project includes a comprehensive test suite:

```bash
# Run all tests
cargo test --test test -- tests --show-output

# Testing with U-Boot development board
cargo test --test test -- tests --show-output --uboot
```

## References

- [Intel 82576EB Gigabit Ethernet Controller Datasheet](https://www.intel.com/content/dam/www/public/us/en/documents/datasheets/82576eg-gbe-datasheet.pdf)
- [Intel IGB Driver Source Code](https://github.com/torvalds/linux/tree/master/drivers/net/ethernet/intel/igb)
- [smoltcp Network Stack](https://github.com/smoltcp-rs/smoltcp)

## License

This project is licensed under an appropriate open source license. Please see the LICENSE file for details.
