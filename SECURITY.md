# Security

Report vulnerabilities privately through GitHub private vulnerability reporting
(Security -> Report a vulnerability) rather than in public issues.

Design guarantees: `unsafe` is confined to per-platform `*-sys` crates, each block with a
`// SAFETY:` justification and a mandatory clean-context review; all other crates forbid
`unsafe`. The real-time path never allocates, locks or blocks, and a panicking or slow
render source or a removed device must not crash the process.
