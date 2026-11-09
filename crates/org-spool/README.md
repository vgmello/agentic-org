# org-spool

The Phase A local spool and the office's temporary offline fallback file reader.

`office_ingest::Drain::drain` reads only `<office folder>/spool/new/<ULID>.json`. It uses
nonblocking, no-follow opens and checks both lstat and descriptor metadata: regular
file, current effective uid, mode 0600, one hard link, at most 512 KiB. The bounded
read also detects files that grow past the cap. The spool, new and dead directories
must be owned by the current user with mode 0700. Passes serialize with a nonblocking `.meta.lock`. Reuse one drainer across passes.
An owned, regular 0600 file with exactly two links is retried for up to three scans
to allow a link/unlink publisher to finish; it is then quarantined if still linked.
More than two links are refused immediately.
The callback owns message validation and durable delivery; this crate knows no office
message contract or member directory.

Accepted plaintext files are deleted immediately. Refused files move to `spool/dead/`
under a fresh quarantine ULID and are purged after 24 hours, measured from quarantine
time rather than untrusted file timestamps. Transient failures stay in `new/` for
retry. Diagnostics contain only a validated filename id (or `invalid`) and a fixed
reason. The server drains synchronously before serving clients and scans every five
seconds. Nothing under `inbox/` is purged by office ingest.

Only active agents run by hand may spool sends (no run mode). A send with `reply_to`
uses normal send visibility rules; it does not claim a held ask as `msg.reply` does.
