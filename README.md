# ryu-predict

Ryu Predict — a Windows-only experimental system-wide predictive-typing companion (Win32/UIA): inline ghost text in any text field, accepted with Tab. Compile-only, unshipped; mac/linux planned.

> **Read-only mirror.** Developed in https://github.com/amajorai/ryu —
> please open issues and pull requests there, not on this repository.

## Status: Windows-only, experimental, unshipped

The **source of record** for Ryu Predict, a **Windows-only experimental**
companion (Win32 / UIA). It is **compile-only and unshipped** today — it is
not built into any release. **mac / linux are planned.** Standalone Cargo
crate (`cargo build` on Windows); Core owns the prediction brain
(`/api/predict/complete`), so no model id or Gateway policy lives here.

## License

Apache-2.0 — see [LICENSE](./LICENSE).

---

# <img src="https://raw.githubusercontent.com/amajorai/ryu/main/.github/logo.png" width="50" align="middle" alt="" />&nbsp; Ryu Predict

> The system-wide predictive-typing companion: inline ghost text in any text field. Part of [Ryu](../../README.md).

[![License](https://shieldcn.dev/badge/License-Apache--2.0-73DC8C.svg?logo=apache&logoColor=white)](../../README.md#repository-layout--licensing)
[![Stack](https://shieldcn.dev/badge/Rust-Win32%2FUIA-dea584.svg?logo=rust&logoColor=white)](../../README.md)

Inline ghost-text autocomplete in ANY text field, accepted with Tab. It is the productized form of [`apps-store/predict-spike`](../predict-spike/README.md). The spike's proven primitives (UIA caret rect + context, the Tab-swallow keyboard hook, `SendInput` injection) are lifted here and tied to Core's `/api/predict/complete` brain, which routes every prediction through the Gateway. The companion stays deliberately dumb: it reads the caret context, asks Core, and renders the reply, so no model id, Gateway URL, or policy ever lives in this process. Standalone Cargo crate (no `package.json`); Windows-first.

**Tier:** OSS, Apache-2.0

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
