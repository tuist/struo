# Unchanged Builds

Learn how Struo returns quickly when a build has nothing to do, and which
environment variables to set in one-shot containers or when debugging reuse.

Most builds have nothing to do. Struo records what each build read and produced,
and on the next build it checks whether any of it changed before deriving,
analysing, or running anything again.

## How it works by default

The first time Struo builds in a working copy, it starts a small background
change tracker that watches sources and build outputs. While the tracker runs,
an unchanged build only has to ask it whether anything moved, so building an
unchanged target in a large workspace takes a few tens of milliseconds.

The tracker exits on its own when the working copy goes quiet. The next build
after that is slower while the tracker starts again.

Without a tracker, Struo compares the size, mode, modification time, and inode
change time of every file the previous build read or produced. It gives the
same answer, but the cost grows with the size of the workspace rather than with
the size of the change. Because the operating system updates the inode change
time on every write, files restored with old timestamps (by `rsync --times` or
an archive, for example) are still detected as changed.

On top of that, Struo reuses earlier work when nothing it depends on changed:

- the result of building a target whose files did not move and whose
  dependencies did not rebuild;
- the graph derived from package manifests, so the package manager's resolver
  does not run again;
- the commands a target kind produced, as long as every environment variable,
  tool lookup, version probe, and glob it consulted still gives the same answer.

A target whose actions opt out of caching is never reused this way.

## Skip the change tracker

In a one-shot container there is no series of builds to pay back the tracker's
startup, so skip it:

```sh
STRUO_CHANGE_TRACKER=0 struo build app
```

Struo then compares file metadata instead. `false` works in place of `0`.

## Turn off reuse while debugging

Each of these settings makes Struo redo one kind of work on every build. Use
them to confirm that a result came from fresh work rather than from a record of
earlier work:

| Variable | Effect when set to `0` or `false` |
| --- | --- |
| `STRUO_TREE_DIGEST_CACHE` | Reads every dependency tree in full instead of comparing file metadata. |
| `STRUO_TARGET_OUTCOMES` | Visits every target instead of reusing recorded target results. |
| `STRUO_RESOLUTION_CACHE` | Derives the graph from package manifests every time. |
| `STRUO_ANALYSIS_MEMO` | Runs target kind analysis every time to produce commands. |

```sh
STRUO_TARGET_OUTCOMES=0 struo build app
```

Expect these builds to be slower. They do not change action caching: actions
whose inputs are unchanged are still restored from the cache.

## Limitations

- The first build in a working copy, and the first build after the tracker
  exits, pays the tracker's startup cost.
- A target kind that consults something Struo cannot record is analysed again
  on every build.

## Next steps

- [Memory limits](/guide/local-execution/memory-limits): control how much
  local work runs at once.
- [Caching scripted workflows](/guide/scripted/caching): declare what
  invalidates a script's cached result.
- [Typed Graph guide](/guide/graph/): build targets that benefit from this
  reuse.
