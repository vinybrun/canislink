# Contributing

1. Read `docs/architecture/canislink-system-architecture.md`.
2. Follow the PR plan (PR-01 onward); keep PRs independently reviewable.
3. Session protocol changes must update `crates/protocol` + `crates/session` tests together.
4. Never add a human accept/approve step on the dog session path.

## Humans out of session path

Steward APIs may: claim terminals, billing, `emergency_stop`, `social_disabled`, bond bootstrap.

Steward APIs must not: accept invites for dogs, force session start without dog engagement, or require owner push notification to complete accept.
