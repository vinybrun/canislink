# Anticipated gaps loop

| Gap | Built | Verified |
|-----|-------|----------|
| Offline / not present peer | fail_paths | yes |
| Caller not present | fail_paths | yes |
| Sleep / rate limit | fail_paths | yes |
| Busy invite | fail_paths | yes |
| Phone as dog AV hardware | Android WebView + portal-web | ANDROID_E2E_OK |
| Invite push not poll | `/v1/ws` DeviceEvent hub | device_ws_push e2e |
| getUserMedia on http WebView | canvas lab stream fallback | log: LAB canvas stream |
| WebRTC on Android portal | signal open + autostart answerer | logcat |
| Dual real phones | Android + host Chromium | yes (not dual emulator) |

## Still open

- Second Android AVD peer (memory/CPU heavy)
- Real camera on emulator (use host fake media / canvas)
- Device WS stability from WebView (reconnect loop; prefer wss in production)
- TURN for hard NAT
