# MCU ↔ SBC UART protocol (v1)

Baud: **115200 8N1**

## Frame

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 | Magic0 `0xCA` |
| 1 | 1 | Magic1 `0x15` |
| 2 | 1 | `msg_type` |
| 3 | 1 | `payload_len` |
| 4 | N | payload |
| 4+N | 1 | XOR checksum of bytes `[2 .. 4+N)` |

## Message types

| Type | ID | Direction | Payload |
|------|----|-----------|---------|
| Sense | `0x01` | MCU→SBC | 8 bytes: force_deci_n le16, tof_mm le16, flags u8, pad[3] |
| Button | `0x02` | MCU→SBC | 4 bytes: pad u8, event u8, hold_ms le16 |
| Led | `0x10` | SBC→MCU | implementation-defined |
| Ping/Pong | `0x7E`/`0x7F` | either | empty |

Reference implementation: `crates/protocol/src/mcu.rs`, emulator `firmware/mcu-emu`.
