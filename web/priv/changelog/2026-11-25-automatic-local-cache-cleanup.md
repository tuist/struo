---
title: The local cache cleans itself up
date: 2026-11-25
---

Struo now keeps the local cache under a size cap of 20 GB on its own. At most
once an hour, a command that runs actions, such as `struo build`, `struo test`,
or `struo exec`, starts a cleanup in the background that removes the least
recently used results when the cache is over the cap. Every build that relies
on a result counts as a use, including builds that skip their actions because
nothing changed. Nothing used within the last hour is removed, so builds
running at the same time are unaffected, and anything removed runs again when
it is next needed.

Change the cap with `max_size` under `[local_cache]` in the root `struo.toml`,
or with the `STRUO_CACHE_MAX_SIZE` environment variable, which takes
precedence. Both accept a size such as `50GB`, and `0` turns automatic cleanup
off. `struo cache stats` reports usage against the cap, and `struo cache gc`
reclaims down to it on demand, or to 20 GB when automatic cleanup is off.
