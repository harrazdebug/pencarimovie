# `legacy/` — superseded implementations

Nothing in this directory is built, released or deployed. The product is the Rust
server at the repository root (published as `pencarimovie-server-<os>-<arch>`);
`public/` is the dashboard it embeds.

| Directory     | What it is                                                                 | Status                                                                                                                                |
| ------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| [`php/`](php) | The PHP + FrankenPHP + MadelineProto implementation (the original engine). | Archived. Not in any release feed, not deployed. **Tracked.**                                                                         |
| `ts/`         | An early TypeScript re-implementation attempt.                             | Dead. Never reached feature parity. **Present on disk but git-ignored** (it carries a `node_modules` tree and nothing depends on it). |

## Why these are kept instead of deleted

The PHP implementation is the reference for a lot of behaviour the Rust port had to
match (catalog gating, the stream matcher, the auth token model, the ALAC handling).
The port audit (PARITY.md / PORT_GAPS.md — kept locally, not published) refers back
to it, so deleting it would break those references. Git history alone is not enough
for something that is still used as a specification.

If you are looking for how to install or run the server, you do not want anything in
this directory — see the root [`README.md`](../README.md).
