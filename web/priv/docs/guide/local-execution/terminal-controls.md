# Terminal Integration

Struo reports build, test, lint, and run status to your terminal's tab or
notifications, links to live run dashboards, and redraws progress without
flicker.

All of this is automatic on interactive terminals and works without an account
or network connection. Terminals that do not support a feature ignore it.

## Status in your terminal

Struo reports its status using the
[Program Status Protocol (OSC 7501)](https://www.superlogical.com/rex/docs/build/program-status).
A supporting terminal can show that work is running, finished, failed, or
cancelled without reading progress spinners or command output. The terminal
decides whether to show a tab indicator, a notification, or another view.

Struo reports preparation as working, success as done, and failure as error.
It does not report progress percentages, and it does not report scheduler or
memory waits as requests for your input.

## Open a live run

When a reporting service creates a build, test, or script run and returns its
dashboard URL, Struo prints the link immediately:

```text
  ↗ Struo live: https://dashboard.example/runs/123
```

On an interactive terminal, the URL is an
[OSC 8 hyperlink](https://ghostty.org/docs/vt/osc/8) you can click. In
redirected logs and machine-output modes it is plain text. The link is printed
once and stays above the progress panel. For a fast cached run, it can appear
after the completion summary.

## Synchronized redraws

On supporting terminals, Struo draws each progress-panel update as one frame
(DEC private mode 2026), so the panel does not flicker or tear. This affects
only drawing, never execution.

## Turn off terminal controls

Terminal controls are on for human output on an interactive terminal. They are
off for redirected output, machine-output formats, `TERM=dumb`, and detected CI
environments.

To turn off status reports, hyperlinks, and synchronized redraws:

```sh
struo --terminal-controls=never build App
```

The default is `--terminal-controls=auto`. The setting is independent of color:
`--color=never` and `NO_COLOR` turn off colors, not terminal controls.
`--quiet` hides the progress panel and dashboard link but keeps the invisible
status reports.

## Limitations

- Literal commands run through `struo exec` do not send Struo status reports,
  so a child command's own status sequences are not confused with Struo's.
- Output from child processes shown in the progress panel has terminal escapes
  removed. The original output stays unchanged in the cache, and `struo run`
  and `struo exec` pass application output through as is.
- When a run is interrupted, Struo resets the status to idle before exiting only
  for runs connected to a reporting service. Otherwise the terminal clears the
  status when the process exits or the next prompt appears.
- On Windows, terminal controls also require virtual-terminal support.

## Next steps

- [The Sound of Your Build](/guide/local-execution/sound): hear run status
  instead of watching it.
- [Memory limits](/guide/local-execution/memory-limits): control how much local
  work runs at once.
- [Coding harnesses](/guide/harness#follow-build-and-test-progress): follow
  build and test progress from a script or agent through a progress file.
