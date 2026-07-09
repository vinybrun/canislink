# Session status indicators

| UX state | LED |
|----------|-----|
| IdlePresent | soft white breath |
| RingingOut | amber slow |
| RingingIn | blue pulse (lure) |
| InSession | solid green both consoles |
| Ending | fade out |

Controlled by SBC → MCU `Led` frames (`0x10`).
