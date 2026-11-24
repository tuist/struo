# The Sound of Your Build

Turn on `--sound` to hear a Struo build or test run as generative music, so you
can tell cache hits, fresh work, and failures apart without watching the
terminal.

Watching a build for status messages is a poor use of your attention. With
`--sound`, each run becomes one continuous piece that changes with what the run
is doing.

## Turn on sound

Sound is off by default. Add `--sound` to any command that runs actions
(`build`, `test`, `run`, `exec`, `lint`, and the compatibility wrappers for
`xcodebuild`, `swift`, `bazel`, and `cargo`) to turn it on for that invocation:

```sh
struo --sound build services/api/Api
struo --sound test --all
struo --sound exec -- mise install --yes
```

On Linux, audio output goes through ALSA, and `struo` loads it whether or not
you pass `--sound`. Minimal images such as `debian:*-slim` do not ship it, so
install `libasound2` there before running `struo`:

```sh
apt-get update && apt-get install -y libasound2
```

## What you hear

### Each command has its own piece

Struo derives a musical identity from the command (the verb, the target, or the
arguments of a low-level `exec`). The same command always gets the same chord
progression, tempo, and register, so `struo build services/api/Api` always
sounds like itself and `struo test --all` sounds different. Every piece stays in
C major.

### A pad that follows the run

Underneath is a slowly evolving pad: a deep drone, a mid-range chord walk, and an
airy shimmer, with a quiet arpeggio that quickens when the build is busy and
slows when it goes quiet. Events reshape the pad for the next fifteen to twenty
seconds:

- **Cache hit:** a ringing bell chord with a high sparkle. The pad gets brighter.
- **Fresh action:** a short brass stab. The pad grows warmer and thicker.
- **Failure:** a low, dark thump. The pad sinks darker.

A cache-heavy build glows and rings, a build doing lots of fresh work grows
warm, and a failing build settles low.

### The ending

When the command finishes, the pad releases together with a closing gesture:

- **Success:** a slowly swelling low brass hit under a rising major chord.
- **Cache-hit finish:** if a cache-hit bell rang in the last two seconds, the
  bells are the resolution instead.
- **Failure:** the chord shifts to the relative minor and the pad releases.

Struo waits for the release to finish before it exits, so the last few seconds
reach the speakers.

## Limitations

- When the default audio device is unavailable, such as on headless CI or a
  machine without audio hardware, `--sound` has no effect and the command runs
  normally.
- Sound never affects execution, caching, remote work, exit codes, or output.
- The closing release can add up to about half a minute to very short commands.
  Drop `--sound` when you need a quick exit.

## Next steps

- [Terminal controls](/guide/local-execution/terminal-controls): show run status
  in your terminal's tab or notifications instead.
- [Unchanged builds](/guide/local-execution/unchanged-builds): see why a cached
  build finishes so quickly.
- [Memory limits](/guide/local-execution/memory-limits): control how much local
  work runs at once.
