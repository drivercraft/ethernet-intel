# Ethernet-Intel

[![Test CI](https://github.com/drivercraft/ethernet-intel/actions/workflows/ci.yml/badge.svg)](https://github.com/drivercraft/ethernet-intel/actions/workflows/ci.yml)

## Running Tests

Install `ostool`:

```bash
cargo install ostool
```

Run tests:

```bash
cargo test --test test -- tests --show-output
# Testing with U-Boot development board
cargo test --test test -- tests --show-output --uboot 
```
