# Tuist coverage-selection validation

Run this fixture against a disposable Tuist project whose server supports
Once coverage and the explicit `instrumented` selection policy. It creates a
temporary Git repository, publishes test runs and Git history, then removes
the local repository. Remote history remains in the test project.

Prerequisites: the Struo binary, Go, Git, Bash and jq. The fixture uses local
build caching; its Tuist token needs test/report permissions, not CAS access.
Set `TUIST_TOKEN` through a restricted file or secret environment variable.
Do not put the token on the command line or enable shell tracing.

```sh
export STRUO_BINARY=/absolute/path/to/struo
export TUIST_URL=https://your-tuist-server.example
export TUIST_ACCOUNT=your-account
export TUIST_PROJECT=disposable-selection-project
export STRUO_VALIDATION_OUTPUT=/tmp/struo-selection-results
bash fixtures/test_selection/go_coverage/validate.sh
```

The default plan must retain AlphaTests, BetaTests and NarrowTests. Opt-in
selection must exclude only BetaTests after changing the alpha production
source. NarrowTests deliberately instruments a different package, so another
target's instrumentation cannot justify excluding it. A fresh clone with no
retained results must make the same decision using server history.

Further controls cover selected execution and a second selection, changed test
inventory/configuration, added/deleted/renamed sources, unavailable service, dynamic
subtests, test failures, aggregate-only coverage, and a full baseline refresh.
Inventory/configuration controls supply the complete diff; incomplete-diff fallback
is checked separately. Both nonempty and fully excluded portable plans must reject
an input edit to an excluded target. The empty plan must execute as a successful
no-op before that edit. Positive selection milestones retry boundedly while the
server projects asynchronous checkpoints.
JSON documents under `STRUO_VALIDATION_OUTPUT` capture each command's result.
An unsupported producer or failed report must keep the test execution verdict
and report `coverage_reported: false`.
