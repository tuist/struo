---
title: Discover Dockerfile.<name> images
date: 2026-11-26
---

Struo now discovers each `Dockerfile.<name>` and `<name>.Dockerfile` as a
`dockerfile_image` target named `<name>_image`, with no `struo.toml`. A
monorepo that keeps one Dockerfile per app at the root, such as
`Dockerfile.web` built with `docker build -f Dockerfile.web .`, gets a
`web_image` target whose build context is the root. A matching
`Dockerfile.web.dockerignore` applies to that image. Several Dockerfiles in one
directory now each get their own target. See
[Container Images](/guide/graph/containers).
