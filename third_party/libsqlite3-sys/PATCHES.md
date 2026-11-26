# Bundled SQLite upgrade

This is the crates.io `libsqlite3-sys` 0.30.1 package, with the SQLite
amalgamation upgraded from 3.46.0 to 3.51.3 and both bundled Rust bindings
regenerated with bindgen 0.69.5 using the upstream build script. The upstream
upgrade script's download URL also points to 3.51.3. Other Rust APIs, build
flags, feature definitions, and SQLCipher sources are unchanged.

The workspace patch keeps rusqlite 0.32.1 and SQLx 0.8 on one native library.
Microsandbox also requires SQLx 0.8, preventing a move to released newer
libsqlite3-sys versions without upgrading that dependency chain.

SQLite 3.51.3 fixes the WAL-reset corruption race affecting 3.46.0:
https://sqlite.org/releaselog/3_51_3.html

Source archive: https://sqlite.org/2026/sqlite-amalgamation-3510300.zip

Published SHA3-256 of `sqlite3.c`, verified before replacement:
`32d5424f97e0a7fc5ed2f6335afbb58be4e0298bd7117a34e39d345ff13d859e`

SQLite source ID:
`2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6d618`

The SQLite sources are public domain; the binding crate retains its upstream
MIT license. This patch upgrades bundled SQLite only, not optional SQLCipher
or user-supplied system libraries. Cargo patches apply to builds from this
workspace, not downstream crates.io consumers of published SDK crates.

Remove this patch when the dependency chain accepts a released binding crate
with SQLite 3.51.3 or later. Preserve the single-library constraint and verify
both rusqlite and SeaORM connections against the replacement.
