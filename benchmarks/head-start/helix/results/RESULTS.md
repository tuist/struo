# Helix: Head Start results

struo 2.24.4 against cargo 1.99.0 (5f94df478 2026-08-27), measured 2026-11-27 on Linux x86_64, 16 CPUs, 8 GiB.
Helix at [`1a3fdfda6dfd`](https://github.com/helix-editor/helix/commit/1a3fdfda6dfdda4567af6093a6249bb2bb422fc6) (2026-11-20), building `cargo_helix_term_bin_hx` with `cargo build --locked --quiet -p helix-term --bin hx`.
Median of 1 runs. Disables Helix's tree-sitter grammar download (HELIX_DISABLE_AUTO_GRAMMAR_BUILD) and sets debug = 0 in the dev profile, which Struo applies only when a profile names it. Both tools use the Rust toolchain mise selects for this repository.

## Build time

| Scenario | cargo | Struo, loopback | Struo, rtt-50ms |
| --- | --- | --- | --- |
| Fresh checkout | 21.2 s | 3.9 s | 24.3 s |
| Fresh checkout, `helix-term` edited | 16.9 s | 7.8 s | 33.7 s |
| Fresh checkout, `helix-core` edited | 21.3 s | 13.1 s | 36.5 s |
| Fresh checkout, no shared cache |  | 40.3 s |  |
| Built checkout, `helix-term` edited | 2.7 s | 8.3 s |  |
| Built checkout, `helix-core` edited | 3.1 s | 13.0 s |  |

## Network profiles and cache traffic

Per fresh-checkout Struo build. Request latency runs from the request reaching the proxy to the last byte of its response.

| Scenario | Profile | Round trip | Bandwidth | Requests | Connections | p50 | p95 | p99 | Down | Up |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Fresh checkout | loopback | 0 ms | unlimited | 2801 | 2801 | 0.73 ms | 9.24 ms | 24.89 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | rtt-50ms | 50 ms | unlimited | 2801 | 2801 | 50.54 ms | 53.4 ms | 58.91 ms | 871.1 MiB | 0 MiB |
| Fresh checkout, `helix-term` edited | loopback | 0 ms | unlimited | 2861 | 2861 | 0.41 ms | 4.37 ms | 10.3 ms | 664.6 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | rtt-50ms | 50 ms | unlimited | 2875 | 2875 | 50.75 ms | 53.01 ms | 56.96 ms | 664.6 MiB | 107 MiB |
| Fresh checkout, `helix-core` edited | loopback | 0 ms | unlimited | 2859 | 2859 | 0.45 ms | 3.73 ms | 13.04 ms | 602.1 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | rtt-50ms | 50 ms | unlimited | 2866 | 2866 | 50.96 ms | 53.37 ms | 58.86 ms | 602.1 MiB | 14 MiB |

## Commit history

The last 20 commits on the first-parent line (2026-11-20 to 2026-11-20), mapped onto the 232-target graph of `cargo_helix_term_bin_hx`:

- 5% change no file of any target in the graph, so a shared cache restores the whole build.
- The median commit invalidates 2 targets and the 90th percentile 8, counting every dependent.
- 0% touch Helix's dependency manifests, which can also change external packages; that is not counted above.

| Target | Commits changing it | Commits invalidating it |
| --- | --- | --- |
| `cargo_helix_term_bin_hx` | 12 | 19 |
| `cargo_helix_term` | 12 | 19 |
| `cargo_helix_core` | 6 | 6 |
| `cargo_helix_tui` | 1 | 9 |
| `cargo_helix_lsp` | 1 | 7 |
| `cargo_helix_view` | 1 | 8 |
| `cargo_helix_loader` | 0 | 0 |
| `cargo_helix_stdx` | 0 | 0 |
| `cargo_helix_parsec` | 0 | 0 |
| `cargo_helix_dap_types` | 0 | 0 |
| `cargo_helix_dap` | 0 | 6 |
| `cargo_helix_lsp_types` | 0 | 0 |
| `cargo_helix_event` | 0 | 0 |
| `cargo_helix_vcs` | 0 | 6 |
