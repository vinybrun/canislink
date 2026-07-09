/* Reference sketch for real MCU port (not built in CI).
 * Frame format must match crates/protocol/src/mcu.rs
 */
#include <stdint.h>

#define MAGIC0 0xCA
#define MAGIC1 0x15
#define MSG_SENSE 0x01

static uint8_t xor_csum(const uint8_t *b, unsigned n) {
  uint8_t c = 0;
  for (unsigned i = 0; i < n; i++) c ^= b[i];
  return c;
}

/* payload: force_deci_n le16, tof_mm le16, flags u8, pad[3] */
void canis_encode_sense(uint16_t force_deci_n, uint16_t tof_mm, uint8_t flags,
                        uint8_t out[13]) {
  out[0] = MAGIC0;
  out[1] = MAGIC1;
  out[2] = MSG_SENSE;
  out[3] = 8;
  out[4] = (uint8_t)(force_deci_n & 0xff);
  out[5] = (uint8_t)(force_deci_n >> 8);
  out[6] = (uint8_t)(tof_mm & 0xff);
  out[7] = (uint8_t)(tof_mm >> 8);
  out[8] = flags;
  out[9] = out[10] = out[11] = 0;
  out[12] = xor_csum(&out[2], 10);
}
