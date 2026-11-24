# Memory Limits

Set a memory budget with `--memory-limit` and CPU slots with `--cpu-limit` so
concurrent local Struo builds and tests fit on constrained or shared machines.

By default, Struo schedules local actions against two thirds of the memory
visible to the host or container. That leaves room for Struo itself, the
operating system, editors, and other processes. On Linux, the default respects
readable cgroup limits, so it follows a container's memory limit instead of the
host total.

## Set a memory limit

Override the budget for any command with `--memory-limit`:

```sh
struo --memory-limit 2GiB build app
```

The value accepts a raw byte count, binary suffixes (`KiB`, `MiB`, `GiB`,
`TiB`), or decimal suffixes (`KB`, `MB`, `GB`, `TB`). It must be greater than
zero.

The option is global, so these forms are equivalent:

```sh
struo --memory-limit 2147483648 test app
struo test app --memory-limit 2147483648
```

## Set a CPU limit

Set `--cpu-limit COUNT` to override the scheduler's CPU capacity:

```sh
struo --cpu-limit 4 test app
```

Test batches divide the parent CPU budget instead of each inheriting the full
host capacity. Configure a tool's own parallelism through its target kind when
needed.

## What to expect

Each local action reserves its declared memory estimate before it starts.
Actions without an estimate reserve 250 mebibytes. Struo starts more work only
when the combined reservations fit within the budget, so a lower limit means
fewer actions at once. An action whose estimate exceeds the entire budget runs
alone instead of waiting forever.

The same budget applies to builds, lint targets, run targets, literal commands,
annotated scripts, and test workers. Nested Struo invocations, such as test
workers and agent tools, inherit the limit instead of resetting to the machine
default. When a remote cache is selected, part of the budget is set aside for
cache transfers, and actions use the remainder.

Large cached outputs are streamed through bounded buffers and temporary files,
so artifact size does not turn into a same-sized memory allocation.

## Choose a value

Start with the automatic default. Set a lower value in a small container or on
a shared machine where other services need predictable headroom. If builds
become unnecessarily serial, raise the limit gradually and declare realistic
memory estimates for unusually small or large actions.

## Limitations

- The limit is an admission budget based on action estimates, not a hard
  operating-system memory ceiling. A command that uses more memory than its
  estimate can still exceed the budget by itself.
- The CPU limit is a scheduling estimate, not a limit on the threads a tool can
  start.
- The budget belongs to one invocation. Independent Struo processes do not
  share a reservation pool, so their combined use can exceed one budget.

When a command must be stopped at an exact memory boundary, use a container or
operating-system process limit, and pair it with a lower Struo budget so the
scheduler leaves headroom for Struo and the operating system.

## Next steps

- [Unchanged builds](/guide/local-execution/unchanged-builds): make repeated
  builds with nothing to do return quickly.
- [Remote Execution](/guide/infrastructure/remote-execution): move heavy
  actions off a constrained machine.
- [CLI reference](/reference/cli/): every global option.
