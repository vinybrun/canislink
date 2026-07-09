# CanisLink Design Document — Production Summary

**Document:** `/work/docs/architecture/canislink-system-architecture.md`  
**Status:** Draft **0.3.0** (implementation-ready)  
**Date:** 2026-07-09  
**Type:** Greenfield founding architecture (software + hardware)

## What was produced

A full, implementation-ready system architecture for **CanisLink**, a dog-to-dog social portal network with standalone terminals and **no humans in the session protocol**. Draft 0.3.0 incorporates two review rounds (display/FSM/identity/TURN/bonds/timezone/IPC/stack/PR plan, then session soft-segment, WebRTC roles, Halted vs social_disabled, single WS, glare re-check, ConfigV1).

## Core architectural stance

- **Dogs control** invite, accept (engage), continue (Again soft-segment), and end (Done / walk-away / SegmentExpired / max).
- **Humans are infrastructure only**: purchase, power, install, billing, emergency_stop, social_disabled — never per-invite approvers.
- **Terminals are standalone** (SBC + MCU, Wi-Fi/LTE; lab reference **CM4 + HDMI**).
- **Matching**: mutual bond graph (`W_MIN=0.30`) + live presence; K=1 ring; glare lex invite_id + presence re-check.
- **Receiver UX**: dog-native lure (LED + short audio), never owner push accept.

## Major sections

Hardware (display first-class), firmware/edge (UX FSM + social_armed, IPC, GStreamer media), session protocol (edge↔cloud map, soft segment, roles), media (P2P+TURN HMAC, single `/v1/ws`), cloud (device mTLS, steward matrix), data model (tz, constraints, audit), welfare/policy, security, observability/operability, monorepo, BOM+lab+welfare-protocol, alternatives, risks, **KD-1..KD-26**, PR-01..PR-29.

## Quantified targets (highlights)

| Item | Target |
|------|--------|
| Invite → lure lab LAN | ≤500 ms p95 (warm WS) |
| Invite → lure prod same-region | ≤800 ms p95 |
| Accept → media lab Eth | ≤2.5 s warm |
| Soft segment initial | 5 min → `SegmentExpired` if no Again |
| Hard max session | 15 min default (`MaxDuration`) |
| Presence TTL / exit | 10 s / 2.5 s |
| W_MIN | 0.30 mutual |
| Scale | 10→1000 terminals; ~5→40→400 concurrent sessions |

## Key decisions beyond product thesis (16–26)

- KD-16 GStreamer media; KD-17 mutual bonds/glare; KD-18 edge owns WS; KD-19 soft-segment Again; KD-20 IANA tz; KD-21 lab CM4+HDMI; KD-22 terminal≠biometric; KD-23 partition P2P continue; KD-24 early dev-CA+ICE; **KD-25 offerer=initiator**; **KD-26 single WS + social_disabled semantics**.

## PR spine

1. PR-01–10: skeleton → protocol → FSM → policy → db → presence → device-auth/dev-CA → bond → compose → device-api config/ICE  
2. PR-11a–c: invite route → accept/end/segment → control WSS  
3. PR-12–16: signaling, steward, policy-worker, sim-dog, e2e  
4. PR-17–21: MCU/sense, edge, media 20a/b/c, health  
5. PR-22–29: BOM, observability, provision, lab+welfare, flags, ADR freeze, loadtest, toy stub  

## Files

| Path | Purpose |
|------|---------|
| `/work/docs/architecture/canislink-system-architecture.md` | Full architecture Draft 0.3.0 |
| `(review archive cleaned)` | Review + round-1/2 revision summaries |
| `/work/docs/architecture/canislink-summary.md` | This summary |
