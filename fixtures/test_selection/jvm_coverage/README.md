# JVM coverage collection

This fixture measures two Kotlin reflection test units with JaCoCo. Both
units cover `Product.kt`; `Unused.kt` remains instrumented with zero hits.

Use Linux with Python 3, strace, Kotlin and a JDK on the executable search
path. From a disposable copy of this directory:

```sh
bash install-tools.sh
struo test KotlinTests --format json
```

The normalized report must have `coverage.partial = false`, two exact
`coverage_evidence.scopes`, and zero covered lines for `Unused.kt`.
Coverage observations bypass the execution cache.

The real Java collector regression checks asymmetric per-unit attribution,
fresh discovery without executing failing bodies, zero-hit sources, spawned
processes and failed tests. Run it from the repository with the installed
fixture tools:

```sh
JACOCO_TOOLS=/absolute/path/to/fixture/tools mise exec -- cargo test \
  -p struo-frontend --test prelude jvm_coverage -- --ignored --nocapture
```

This collector fixture does not claim a hosted Tuist selection run. The Go
fixture covers the shared baseline/decision/portable-plan lifecycle.
