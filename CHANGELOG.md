# Changelog

## 0.2.0-lab — 2026-07-09

**Honest status:** lab-shippable software kit — not a consumer product.

### Lab-shippable software

- **SQLite durability** for enrollments, bonds, policies, invites, sessions (survives API restart)
- **Steward CLI** (`steward enroll|bond|estop|social-disable|health`) — human install path only
- **Real WebRTC**: signaling rooms + `canis-media` peer connections reach `Connected` with `canis-portal` datachannel
- **Lab kit docs**: BOM, install steps, definition of done (`docs/lab/`)
- README no longer claims customer-ready ship

### Still not customer-ready

- Physical terminal manufacturing / field hardware validation
- Camera encode → dog-facing display video
- Production mTLS / multi-tenant cloud
- App store companion UX

## 0.1.0-alpha — 2026-07-09

Control plane prototype (sim + tests). Over-marketed as ship-ready; corrected in 0.2.0-lab.
