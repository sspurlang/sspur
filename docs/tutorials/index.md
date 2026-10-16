# Tutorials

Each tutorial builds one real thing from start to finish. They assume you have read the [handbook](../handbook/index.md), or at least [SSPUR in 5 minutes](../get-started/five-minutes.md). As everywhere in these docs, every file and terminal session is checked by CI against the current compiler.

| Tutorial | What you build | Needs |
|---|---|---|
| [Build a web app](web-app.md) | The reference app from `sspur new web-app`: accounts, notes, SQLite, sessions and CSRF, with its tests and a deploy guide | `clang` |
| [Build and deploy a CRUD service](crud-service.md) | An HTTP service over a table, its IAM policies, and a local run | `clang` |
| [Migrations and hot swap](migrations.md) | A schema change with a migration, a recorded replay, a live swap and a rollback | `clang` |
| [Calling C](ffi.md) | Calls into libm and libc, generated bindings, and a C library built from SSPUR | `clang` |
| [Bare-metal hello](bare-metal.md) | A kernel for QEMU on AArch64 and firmware for a Cortex-M4 | `clang`, `ld.lld`, QEMU to boot |
| [GPU kernels](gpu-kernels.md) | SAXPY and a reduction with shared memory, run on Metal or the CPU | `clang` |
| [A multi-agent codebase](multi-agent.md) | Two replicas edited by two agents, a conflict and its resolution | nothing extra |

The [`examples/`](https://github.com/sspurlang/sspur/tree/main/examples) directory has larger versions of several of these, and [`tests/`](https://github.com/sspurlang/sspur/tree/main/tests) has programs for every feature.
