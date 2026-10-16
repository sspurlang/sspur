# Bare-metal hello

`profile bare` is for kernels, firmware and bootloaders: code that runs with no operating system and no runtime. This tutorial builds one program for three machines and boots it in QEMU.

## The program

```sspur
{{#include ../snippets/tutorials/bare/hello.ssp}}
```

- `profile bare` on the first line turns on the bare rules. There is no garbage-collected heap, so lists, sum types, lambdas, string building, `log` and errors are compile errors (`E_PROFILE_BARE`). Records, tuples, fixed arrays `Array[T, N]`, static strings and integers remain, plus everything from `profile sys`.
- `mmio[U8](addr)` is a volatile hardware register. `.read` and `.write(v)` perform the `mmio` effect, and the polling loops perform `div`, so both are in every signature.
- `arch()` compared with a literal is folded at compile time: each build keeps only its own UART branch.
- Tests run on the host. Pure code like the `bytes` test works as usual.

```console
{{#include ../snippets/tutorials/bare/bare.out:test}}
```

## Build and boot

`sspur build --target` writes freestanding C, a startup file and a linker script, compiles them with clang and links with `ld.lld`. It prints the QEMU command to boot the result:

```console
{{#include ../snippets/tutorials/bare/bare.out:build}}
```

Run that command and the kernel prints `hello from sspur`, then `main` returns, which powers the machine off with exit status 0. The same file builds for a Cortex-M4:

```console
{{#include ../snippets/tutorials/bare/bare.out:m4}}
```

The `riscv64-qemu` target works the same way, but needs a clang with the RISC-V backend: Apple's clang has none, so set `SSPUR_BARE_CC` to Homebrew's LLVM.

## Where to go from here

| Feature | How |
|---|---|
| Interrupts | `fn tick() ! mmio, div` with an `interrupt timer` line after the signature; `timer_start(ticks)`, `wait_irq()` |
| Module state | `static count: Int = 0`, with `count.add(1)`, `.load`, `.cas(old, new)`; each access is atomic with respect to interrupts, and racy read-then-store is `E_STATIC_RACE` |
| Several cores | `start_core(id)` and `fn core_main(id: Int)` on `aarch64-qemu` |
| Traps | Define `fn on_trap(code: Int)`; a trap halts with status 64 + code |
| Inline assembly | `asm "template" in(x: e) out(r: Int) clobber("memory")`, which performs `unsafe` |

The [language reference](../reference/language.md#bare-profile-bare) has the full rules and the memory map of each target, [ADR 0017](../adr/0017-bare-profile.md) the design, and [`tests/bare/`](https://github.com/sspurlang/sspur/tree/main/tests/bare) has a timer, statics, floats and a two-core program.
