# SaveSync

Steam Cloud for any game. A portable, cross-platform app that syncs game saves
between machines using a private GitHub repo as the backend. Works equally well
with cracked games (FitGirl/DODI repacks) and legitimately-bought non-Steam
games — anywhere Steam Cloud doesn't exist.

**Status:** in development. Phase 0 (scaffold + CI) and Phase 1 (sync engine —
manifest, git wrapper, LFS routing, snapshot/diff, conflict resolution, CLI
driver) are landed. Phases 2-5 (detection, onboarding, UI, release) in flight.

See [`docs/artifacts/savesync-spec.html`](docs/artifacts/savesync-spec.html)
for the full v1 spec — locked decisions, architecture, scope.

Built by [Slendy](https://github.com/slendyzo).
