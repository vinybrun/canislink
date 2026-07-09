# Anticipated gaps (from untested scenarios) → what we built

| Untested / missing | Anticipated need | Status in this loop |
|--------------------|------------------|---------------------|
| Phone is not custom hardware display | Use **Android phone as camera+screen** for dog portal | `android/` WebView app + `portal-web/` |
| Real video not just datachannel | Browser WebRTC getUserMedia + remote video | `portal-web/app.js` |
| Emulator networking | Host services via `10.0.2.2` | Documented + portal defaults |
| Peer offline / not present | Route errors | `fail_paths.rs` |
| Caller not on mat | Deny invite | `fail_paths.rs` |
| Sleep / rate limit | Policy denials | `fail_paths.rs` + existing policy |
| Busy concurrent invite | Single open invite | `fail_paths.rs` |
| Human never accepts | Accept only on portal/pad | Enforced in UI + API |
| Serving portal to phone | Static `/portal` on device-api | ServeDir |
| Deep e2e with Android | Emulator + adb | `scripts/android_e2e.sh` |

Still open (later loops): multi-emulator two-phone video, TURN for real NAT, push notifications, Play Store signing.
