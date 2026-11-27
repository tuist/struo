---
title: Remote outputs install into any number of directories
date: 2026-11-27
---

Installing a remote action's outputs no longer fails with "exceeded its pinned
output directories limit of 128". The installer used to keep every output's
parent directory open until it finished, from a pool shared by all concurrent
installs, so large or parallel builds could exhaust it. It now keeps only the
directories it is working in open, whatever the number of outputs.
