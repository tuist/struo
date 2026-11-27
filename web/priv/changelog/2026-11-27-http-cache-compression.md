---
title: HTTP cache downloads negotiate zstd compression
date: 2026-11-27
---

Struo's HTTP cache client now accepts zstd-compressed blob downloads.
`struo cache serve` streams large blobs with zstd when requested, reducing
network traffic without changing cache keys or stored content. Existing
clients still receive raw bytes, and the new client works with older servers.

Compression uses bounded streaming buffers and the server's transfer memory
budget. Downloads still verify the decoded size and content digest before
entering the local cache. Uploads and stored objects retain the existing
uncompressed format.
