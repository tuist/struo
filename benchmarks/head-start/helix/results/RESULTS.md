# Helix: Head Start results

Historical development-build measurements, not current-release results. Source base `6af79d48bf637c491fc1bae890937123200424cc` plus the local Rust admission patch. This run predates the proxy handshake fix and HTTP cache connection/restore changes. The measured proxy did not enforce its extra new-connection delay, so network profiles do not represent the current implementation. Cold builds use no shared cache and are unaffected by that proxy issue.

struo 0.0.0 against cargo 1.99.0 (5f94df478 2026-08-27), measured 2026-11-27 on Linux x86_64, 16 CPUs, 8 GiB.
Helix at [`1a3fdfda6dfd`](https://github.com/helix-editor/helix/commit/1a3fdfda6dfdda4567af6093a6249bb2bb422fc6) (2026-11-20), building `cargo_helix_term_bin_hx` with `cargo build --locked --quiet -p helix-term --bin hx`.
Median of 3 runs. Disables Helix's tree-sitter grammar download (HELIX_DISABLE_AUTO_GRAMMAR_BUILD) and sets debug = 0 in the dev profile, which Struo applies only when a profile names it. Both tools use the Rust toolchain mise selects for this repository.

## Build time

| Scenario | cargo | Struo, loopback | Struo, rtt-10ms | Struo, rtt-50ms | Struo, rtt-150ms | Struo, same-region | Struo, office | Struo, home |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Fresh checkout | 27.1 s | 3.4 s | 7.6 s | 25.7 s | 70.4 s | 4.0 s | 14.9 s | 45.1 s |
| Fresh checkout, `helix-term` edited | 27.7 s | 10.5 s | 15.0 s | 38.1 s | 98.0 s | 13.3 s | 25.9 s | 56.9 s |
| Fresh checkout, `helix-core` edited | 31.1 s | 17.5 s | 21.5 s | 39.0 s | 91.2 s | 17.2 s | 26.7 s | 49.6 s |
| Fresh checkout, no shared cache |  | 25.7 s |  |  |  |  |  |  |
| Built checkout, `helix-term` edited | 3.0 s | 9.2 s |  |  |  |  |  |  |
| Built checkout, `helix-core` edited | 3.7 s | 16.1 s |  |  |  |  |  |  |

## Network profiles and cache traffic

Per fresh-checkout Struo build. Request latency runs from the request reaching the proxy to the last byte of its response.

| Scenario | Profile | Round trip | Bandwidth | Requests | Connections | p50 | p95 | p99 | Down | Up |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Fresh checkout | loopback | 0 ms | unlimited | 2927 | 2927 | 0.66 ms | 6.03 ms | 15.26 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | rtt-10ms | 10 ms | unlimited | 2803 | 2803 | 10.96 ms | 15.86 ms | 26.36 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | rtt-50ms | 50 ms | unlimited | 2801 | 2801 | 51.17 ms | 54.93 ms | 61.05 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | rtt-150ms | 150 ms | unlimited | 2801 | 2801 | 151.7 ms | 157.47 ms | 168.3 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | same-region | 2 ms | 5000 Mbit/s | 2801 | 2801 | 2.97 ms | 9.7 ms | 27.64 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | office | 20 ms | 1000 Mbit/s | 2801 | 2801 | 20.78 ms | 47.96 ms | 134.42 ms | 871.1 MiB | 0 MiB |
| Fresh checkout | home | 40 ms | 200 Mbit/s | 2801 | 2801 | 41.03 ms | 244.02 ms | 837.16 ms | 871.1 MiB | 0 MiB |
| Fresh checkout, `helix-term` edited | loopback | 0 ms | unlimited | 2959 | 2959 | 0.71 ms | 5.89 ms | 14.27 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | rtt-10ms | 10 ms | unlimited | 2959 | 2959 | 10.76 ms | 14.1 ms | 20.52 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | rtt-50ms | 50 ms | unlimited | 2959 | 2959 | 50.92 ms | 54.02 ms | 59.64 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | rtt-150ms | 150 ms | unlimited | 2959 | 2959 | 151.14 ms | 154.75 ms | 160.89 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | same-region | 2 ms | 5000 Mbit/s | 2959 | 2959 | 3.45 ms | 26.42 ms | 51.02 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | office | 20 ms | 1000 Mbit/s | 2959 | 2959 | 21.65 ms | 52.85 ms | 136.48 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-term` edited | home | 40 ms | 200 Mbit/s | 2959 | 2959 | 42.14 ms | 219.94 ms | 825.37 ms | 664.7 MiB | 107 MiB |
| Fresh checkout, `helix-core` edited | loopback | 0 ms | unlimited | 2943 | 2943 | 0.65 ms | 5.52 ms | 17.01 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | rtt-10ms | 10 ms | unlimited | 2901 | 2901 | 10.88 ms | 15.22 ms | 23.88 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | rtt-50ms | 50 ms | unlimited | 2908 | 2908 | 50.94 ms | 54.29 ms | 61.33 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | rtt-150ms | 150 ms | unlimited | 2915 | 2915 | 151.23 ms | 154.91 ms | 161.75 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | same-region | 2 ms | 5000 Mbit/s | 2922 | 2922 | 2.95 ms | 10.21 ms | 24.67 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | office | 20 ms | 1000 Mbit/s | 2929 | 2929 | 20.78 ms | 43.54 ms | 131.24 ms | 602.2 MiB | 14 MiB |
| Fresh checkout, `helix-core` edited | home | 40 ms | 200 Mbit/s | 2936 | 2936 | 41.04 ms | 226.5 ms | 791.92 ms | 602.2 MiB | 14 MiB |

## Commit history

The last 500 commits on the first-parent line (2026-03-14 to 2026-11-20), mapped onto the 232-target graph of `cargo_helix_term_bin_hx`:

- 55% change no file of any target in the graph, so a shared cache restores the whole build.
- The median commit invalidates 0 targets and the 90th percentile 8, counting every dependent.
- 7.6% touch Helix's dependency manifests, which can also change external packages; that is not counted above.

| Target | Commits changing it | Commits invalidating it |
| --- | --- | --- |
| `cargo_helix_term_bin_hx` | 129 | 225 |
| `cargo_helix_term` | 129 | 225 |
| `cargo_helix_core` | 54 | 66 |
| `cargo_helix_view` | 53 | 134 |
| `cargo_helix_lsp` | 14 | 78 |
| `cargo_helix_loader` | 13 | 16 |
| `cargo_helix_tui` | 13 | 145 |
| `cargo_helix_dap` | 10 | 77 |
| `cargo_helix_lsp_types` | 5 | 8 |
| `cargo_helix_stdx` | 4 | 4 |
| `cargo_helix_event` | 4 | 4 |
| `cargo_helix_vcs` | 4 | 72 |
| `cargo_helix_dap_types` | 3 | 3 |
| `cargo_helix_parsec` | 0 | 0 |
