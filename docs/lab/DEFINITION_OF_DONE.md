# Definition of done

## Lab-shippable software (0.2.0-lab)

Call **lab-shippable software** only when all are true:

1. [x] `cargo test --workspace` green  
2. [x] Durable store survives API restart (SQLite)  
3. [x] Steward can enroll, bond, e-stop without architecture docs  
4. [x] Dual-dog control plane sim (`sim-dog --scenario session`)  
5. [x] Two `canis-media` processes reach WebRTC `Connected` + portal datachannel (`scripts/lab_webrtc.sh`)  
6. [x] Written kit BOM + install steps (`docs/lab/LAB_KIT.md`)  

## Customer-ready (not done)

7. [ ] Two physical terminals validated in two rooms with real dogs  
8. [ ] Camera video rendered for dogs (not only datachannel proof)  
9. [ ] Production device identity (mTLS), OTA, support model  

Until 7–9: **do not** say ready for customers.
