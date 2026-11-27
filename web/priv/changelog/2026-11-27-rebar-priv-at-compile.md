---
title: Rebar dependencies read their own priv while compiling
date: 2026-11-27
---

Rebar dependencies whose compilation reads files from their own `priv`
directory build again. Since Struo started keeping the files compile hooks
write into the application's `priv`, that directory was empty while Rebar
ran, so packages such as `certifi`, whose parse transform embeds the
certificate bundle from `priv`, failed to compile. The package's own `priv`
is now in place before Rebar starts, and files hooks write there are still
kept.
