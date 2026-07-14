# Big Bird — Firefox prototype

A demonstration that pdslib (the Private Data Service that Big Bird builds on)
runs inside a real browser: a modified **Firefox** whose Private Attribution API
is backed by pdslib, plus a small extension that visualizes the on-device
privacy-filter state.

**This is a deployability demonstration, not part of the figure reproduction.**
Nothing here is needed to reproduce the paper's figures — that pipeline is pure
Rust + Python (see the [top-level README](../README.md)). This prototype is a
separate, heavyweight, optional component.

## Demo

Screen recording of the pdslib privacy-filter dashboard running in the modified
Firefox:


https://github.com/user-attachments/assets/6d4b0950-d82b-42d0-91c8-fbefd3ecee6f

## Components (pinned pointers, not cloned by default)

Both are git submodules recorded **only as pinned pointers**. A normal
`git clone` of this artifact does **not** download them; initialize them
explicitly only if you want to build the browser prototype.

| Submodule | Repo | Pinned commit | What it is |
| --- | --- | --- | --- |
| `pdslib-firefox` | [columbia/pdslib-firefox](https://github.com/columbia/pdslib-firefox) | `08a07e3` | Firefox 138.0.4 with its Private Attribution API reimplemented on pdslib (~913 MB source tree) |
| `pdslib-firefox-extension` | [columbia/pdslib-firefox-extension](https://github.com/columbia/pdslib-firefox-extension) | `6a31553` | A WebExtension privacy dashboard that plots the filter states |

The actual contribution is a thin patch on stock Firefox — the fork is just
`Import Firefox v138.0.4` → `Import pdslib` → `Implement pdslib-backed
PrivateAttribution` → three small fixes. Almost all of it lives under
[`dom/privateattribution/`](https://github.com/columbia/pdslib-firefox/tree/main/dom/privateattribution)
(notably `nsIPrivateAttributionPdslibService.idl` and the vendored
`dom/privateattribution/pdslib/`).

> **pdslib version.** The browser build embeds a pdslib snapshot vendored on
> **2025-05-24**, predating the evaluation engine's pinned `pdslib` (the
> `quota-count` branch used by `bigbirdeval/`). The prototype is a
> proof-of-concept of *deployability*; its embedded pdslib is intentionally
> independent of, and older than, the version the eval measures. Do not expect
> browser behavior to match the eval's exact pdslib revision.

## Building & running the prototype

Expect a **large, slow build**: compiling Firefox from source needs several
hours, ~30 GB of disk, and a lot of RAM. Follow Mozilla's build docs; the steps
below are the summary.

```bash
# 1. Fetch just this prototype's submodules (skipped by a normal clone)
git submodule update --init firefox-prototype/pdslib-firefox \
                            firefox-prototype/pdslib-firefox-extension

# 2. Build the modified Firefox (see firefox-source-docs.mozilla.org)
cd firefox-prototype/pdslib-firefox
./mach bootstrap        # one-time: installs the Firefox build toolchain
./mach build            # the multi-hour compile
./mach run              # launch the freshly built browser
```

In the running browser:

1. Settings → enable **"Privacy-preserving ad measurement"**.
2. `about:config` → set `dom.origin-trials.private-attribution.state` to `1`.
3. `about:debugging` → **This Firefox** → **Load Temporary Add-on** → pick any
   file in `firefox-prototype/pdslib-firefox-extension/` (e.g. `manifest.json`).

The dashboard opens from the puzzle-piece (extensions) icon and shows the
per-site privacy-filter budgets as attribution events are processed.

## Reproducibility notes

- The two submodules are pinned to fixed commits, so the prototype is
  archivable alongside the rest of the artifact (e.g. on Zenodo) without
  vendoring ~1 GB of Firefox source into this repository.
- Because a full Firefox build is impractical for many reviewers, a short
  screencast / screenshots of the dashboard should accompany the archived
  artifact (see the extension repo's README for a reference screenshot).
