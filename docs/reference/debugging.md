# Debugging

SSPUR programs compile to native code through C, and a debug build maps that code back to your `.ssp` file, so lldb and gdb work at the source level: breakpoints by `file.ssp:line` or by function name, a backtrace of SSPUR functions and lines, `next` and `step` by source line, and locals under their SSPUR names with readable values. The design is in [ADR 0036](../adr/0036-source-level-debugging.md).

## The short way

```console
$ sspur debug prog.ssp arg1 arg2
```

This builds `prog.ssp` with debug info, then starts lldb (macOS) or gdb (Linux) on it with the SSPUR formatters loaded, a breakpoint on `fn main` and one on `sspur_trap`, and runs it. You are at the debugger's prompt, stopped in `main`. `--debugger lldb` or `--debugger gdb` picks one. If neither is installed, it tells you how to get one: on macOS `xcode-select --install`, on Debian or Ubuntu `sudo apt install gdb`.

Inside a codebase (`.sspur/`), run `sspur debug` without a file.

## Debug builds

`--debug` works with `run`, `test`, `build` and `native`, and `SSPUR_DEBUG=1` turns it on for every command:

```console
$ sspur build --debug prog.ssp
built 3 native functions with debug info into /Users/me/.cache/sspur/native/3985bbe4....dylib
source lines refer to /Users/me/src/prog.ssp
debug with: sspur debug prog.ssp   (or: lldb -- sspur run --debug prog.ssp)
$ sspur run --debug prog.ssp
```

A debug build is compiled at `-O0` with frame pointers and DWARF (CodeView on Windows), and the proven rewrites are off, so every line and every local is there. It runs several times slower than a normal build (about 10 times on compute-heavy code). Debug and normal builds are cached separately, and normal builds are exactly what they were before debug builds existed.

The program runs inside the `sspur` process, which loads the native library when it starts the program. That is why the debugger's target is `sspur run --debug prog.ssp`, and why lldb first says a breakpoint has "no locations (pending)": it resolves as soon as the library loads.

## lldb by hand

```console
$ lldb -o "command script import ~/.cache/sspur/debug/sspur_lldb.py" -- sspur run --debug prog.ssp
(lldb) breakpoint set -f prog.ssp -l 12
(lldb) run
(lldb) bt
(lldb) frame variable
(lldb) sspur-locals
(lldb) next
```

A session on a small program looks like this:

```console
(lldb) breakpoint set -f prog.ssp -l 12
Breakpoint 1: no locations (pending).
(lldb) run
1 location added to breakpoint 1
* thread #2, stop reason = breakpoint 1.1
    frame #0: 9da2....dylib`total(xs=[1, 2, 3] len=3, st=0x00000003200020c0, depth=-999998) at prog.ssp:12
   9   	fn total(xs: List[Int]) -> Int = do
   10  	  var acc = 0
   11  	  for x in xs do
-> 12  	    acc := acc + x
   13  	  acc
(lldb) bt 3
  * frame #0: 9da2....dylib`total(xs=[1, 2, 3] len=3, st=0x00000003200020c0, depth=-999998) at prog.ssp:12
    frame #1: 9da2....dylib`f_main(st=0x00000003200020c0, depth=-999999) at prog.ssp:20
    frame #2: 9da2....dylib`sspur_entry_main(st=0x00000003200020c0) at e7ae....c:208
(lldb) frame variable
(L_I) xs = [1, 2, 3] len=3 {
  [0] = 1
  [1] = 2
  [2] = 3
}
(Status *) st = 0x00000003200020c0
(int64_t) depth = -999998
(int64_t) acc = 0
(int64_t) x = 1
```

`st` and `depth` are the runtime's status and recursion depth, passed to every function. `sspur-locals` leaves them out.

## gdb by hand

```console
$ gdb -ex "source ~/.cache/sspur/debug/sspur_gdb.py" --args sspur run --debug prog.ssp
(gdb) set breakpoint pending on
(gdb) break prog.ssp:12
(gdb) run
(gdb) bt
(gdb) info locals
(gdb) sspur-locals
```

## Names

Functions, parameters and locals have their SSPUR names where C allows it:

- A function is called by its own name (`break total`, `breakpoint set -n total`), unless that name is taken in C: `main`, a C library name such as `round`, `free` or `time`, or a name of the runtime. Those keep an `f_` prefix, so `break f_main`.
- A local or parameter has its own name. A second binding of the same name in one function (`let x = x + 1`) is `x__2`, and so on.
- Lambdas are named after the function they are in, like `total__lambda$k0`.
- The values a lambda captures are the fields `c0`, `c1`, ... of `env_`.

## Formatters

`sspur debug` loads them for you. To have them in every session:

```console
$ sspur debug-init
wrote /Users/me/.cache/sspur/debug/sspur_lldb.py and /Users/me/.cache/sspur/debug/sspur_gdb.py
add to ~/.lldbinit:  command script import "/Users/me/.cache/sspur/debug/sspur_lldb.py"
add to ~/.gdbinit:  source /Users/me/.cache/sspur/debug/sspur_gdb.py
$ sspur debug-init --install
```

| Value | Shown as |
|---|---|
| `Int`, `F64` | the number |
| `Bool` | `true` or `false` |
| `Str` | `"hello"` (long strings are cut after 4096 bytes) |
| `List[T]` | `[1, 2, 3] len=3`, with the elements as children |
| `Opt[T]` | `some(3)` or `none` |
| records | their fields, `(x = 1, y = 2)` |
| sum values | `Circle{r: 2}`, `Leaf`, with the variant's fields as children |

Maps, sets and the other standard containers show as their C structs for now.

## Stopping on a trap

Every trap (overflow, division by zero, a failed `pre` or `post`, an index out of range, a failed match, `.unwrap` of `none`) calls `sspur_trap` first:

```console
(lldb) breakpoint set -n sspur_trap
(lldb) run
(lldb) up
frame #1: ...`safe_div(a=12, b=0, st=..., depth=-999998) at trap.ssp:2
-> 2   	  pre b != 0
```

In gdb, `break sspur_trap`. `sspur debug` sets this breakpoint for you. The trap's arguments say what failed: `c` is the kind, `cl` the clause.

## VS Code

With the [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb) extension, put this in `.vscode/launch.json`, with `program` set to where `sspur` is (`which sspur`; the install script puts it in `~/.local/bin`), run `sspur debug-init` once, and start *SSPUR: debug this file* with the `.ssp` file open:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "SSPUR: debug this file",
      "program": "${env:HOME}/.local/bin/sspur",
      "args": ["run", "--debug", "${file}"],
      "cwd": "${fileDirname}",
      "initCommands": ["command script import ~/.cache/sspur/debug/sspur_lldb.py"]
    }
  ]
}
```

The SSPUR extension lets you set breakpoints in `.ssp` files. With an older version of it, or without it, set `"debug.allowBreakpointsEverywhere": true`.

## Windows

Debug builds on Windows carry CodeView, and the linker writes a PDB next to the native library in `%LOCALAPPDATA%\sspur\native`. In Visual Studio, open `sspur.exe` as a project (File, Open, Project/Solution), set its arguments to `run --debug prog.ssp`, open `prog.ssp` and set breakpoints, then start debugging; in WinDbg, `windbg sspur.exe run --debug prog.ssp`, then `bu sspur_trap` or a source-line breakpoint such as ``bu `prog.ssp:12` `` (`bu` keeps it pending until the library loads). lldb from LLVM works too. The formatters are for lldb and gdb only.

## Codebases

For a codebase in `.sspur/`, a debug build writes the projected source to `~/.cache/sspur/debug/src/<hash>/<name>.ssp`, where `<name>` is the codebase's directory, and the lines refer to that file. `sspur build --debug` prints its path. Set breakpoints in it, for example `breakpoint set -f myproj.ssp -l 40`.
