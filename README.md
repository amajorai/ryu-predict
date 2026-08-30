<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./icon-dark.png" />
    <img src="./icon-light.png" alt="Autocomplete" width="144" />
  </picture>
</p>

<div align="center">

# Autocomplete

</div>

A Windows-only experimental system-wide predictive-typing companion (Win32/UIA): inline ghost text in any text field, accepted with Tab. Compile-only, unshipped; mac/linux planned.

> **The public home of `ryu-predict`.** Source, builds, and releases live here —
> binaries for every platform are attached to each release.
>
> This tree is generated from the Ryu monorepo, so commits pushed here
> directly are replaced on the next sync. **Pull requests are welcome** —
> open them here and they are ported into the monorepo, then flow back out.
> Ryu as a whole: https://github.com/amajorai/ryu

## Install

**App:** [Install](ryu://apps/@ryu/predict) (opens the Ryu desktop app and asks you to confirm)

**CLI:**

```bash
ryu apps add @ryu/predict
```

## Status: Windows-only, experimental, unshipped

The **source of record** for Ryu Predict, a **Windows-only experimental**
companion (Win32 / UIA). It is **compile-only and unshipped** today — it is
not built into any release. **mac / linux are planned.** Standalone Cargo
crate (`cargo build` on Windows); Core owns the prediction brain
(`/api/predict/complete`), so no model id or Gateway policy lives here.

## License

Apache-2.0 — see [LICENSE](./LICENSE).

## Build / Run

```bash
cargo check --manifest-path apps-store/predict/Cargo.toml   # verify
cargo run   --manifest-path apps-store/predict/Cargo.toml   # Windows, with a display
```

On non-Windows targets it compiles to a clear "unsupported on this OS" stub.

Connection (matches the other Ryu clients):

- `RYU_CORE_URL`: local Core node (default `http://127.0.0.1:7980`)
- `RYU_TOKEN`: shared node token (optional for a tokenless local node)

## What it provides

- **Caret probe** (`caret.rs`): UIA `TextPattern` caret rect + preceding-text context, the input to each prediction.
- **Tab-swallow hook** (`hook.rs`): a `WH_KEYBOARD_LL` hook gated on a suggestion-visible atomic, so Tab is normal the rest of the time and accepts the ghost text when one is shown.
- **Layered overlay** (`overlay.rs`): a click-through, color-keyed GDI overlay that renders the ghost text at the caret.
- **Engine loop** (`engine.rs`): polls the caret, debounces, and POSTs the context to Core via the `client.rs` `CoreClient`; `SendInput` injection on accept reuses `ghost-hands`.

## Role / How it fits

Core is the brain (`apps/core/src/predict/`): it enforces the privacy denylist + per-app allowlist and hands the call to the Gateway. This companion is the system-wide *surface*, while the in-desktop editor copilot lives in the desktop app instead. The window tech (raw Win32 here) is swappable; the engine is the load-bearing part.

## License

Apache-2.0. See [LICENSE](../../README.md#repository-layout--licensing). © 2026 A Major Pte. Ltd.
