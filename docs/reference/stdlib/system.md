# Files and processes

Everything that touches the outside world is behind an effect: `fs` for files, `io` for stdin and stderr, `proc` for child processes, `time` for the clock and `env` for the environment. A function's signature lists the ones it uses. Calls that can fail return a `Res` with a short error string.

## Calls

| Call | Result |
|---|---|
| `read_file(path)` | `Res[Str, Str] ! fs` |
| `write_file(path, s)`, `append_file(path, s)`, `remove_file(path)` | `Res[Unit, Str] ! fs` |
| `list_dir(path)` | `Res[List[Str], Str] ! fs` (sorted names) |
| `read_line()`, `read_lines()` | `Opt[Str]`, `List[Str]` `! io` (stdin, without the line ending) |
| `now_ms()`, `mono_ns()`, `sleep_ms(n)` | Unix milliseconds, a monotonic nanosecond clock, a pause `! time` |
| `env_var(name)`, `args()` | `Opt[Str]`, `List[Str]` `! env` (`sspur run file.ssp a b` gives `["a", "b"]`) |
| `read_bytes(path)`, `write_bytes(path, bs)` | `Res[List[Int], Str]`, `Res[Unit, Str]` `! fs` (bytes outside `0..255` give `byte out of range`) |
| `mkdir(path)`, `mkdir_all(path)`, `remove_dir(path)`, `rename(from, to)` | `Res[Unit, Str] ! fs` (`mkdir_all` creates parents and accepts existing directories) |
| `exists(path)`, `is_dir(path)`, `file_size(path)`, `modified_ms(path)` | `Bool`, `Bool`, `Res[Int, Str]`, `Res[Int, Str]` `! fs` |
| `copy_file(from, to)` | `Res[Unit, Str] ! fs`: contents and mode bits; `same file` when both name one file, `is a directory` for a directory source |
| `symlink(target, link)`, `read_link(path)`, `is_symlink(path)` | `Res[Unit, Str]`, `Res[Str, Str]` (`not a symlink` for other files), `Bool` (does not follow the link) `! fs` |
| `file_mode(path)`, `set_mode(path, mode)` | `Res[Int, Str]` (`st_mode & 0o7777`, following links), `Res[Unit, Str]` (`mode out of range` outside `0..=4095`) `! fs` |
| `eprint(s)` | `Unit ! io` (stderr, with a newline) |
| `now()` | `Time ! time` |
| `run_cmd(prog, args, input)` | `Res[(Int, Str, Str), Str] ! proc`: runs `prog` (searched in `PATH`) with `input` on stdin and gives the exit status (128 + signal when killed), stdout and stderr; `"prog: not found"` style errors |
| `exit(code)` | `Unit ! proc`: ends the program with that status |
| `proc.spawn(prog, args, env)` | `Res[Proc, Str] ! proc`: starts `prog` (searched in `PATH`) with the extra environment pairs `env`, without waiting for it; stdin is empty and stdout and stderr are read line by line |
| `p.line(timeout_ms)` | `Opt[Str] ! proc`: the next line `p` printed, or `none` at the end of its output or after `timeout_ms` |
| `p.running()` | `Bool ! proc`: whether `p` has not exited yet |
| `p.stop()`, `p.wait()` | `Int ! proc`: the exit status; `stop` sends SIGTERM and, after 10 seconds, SIGKILL (Windows terminates the process at once), `wait` waits. `p.pid` is the process id |

A spawned process that is still running when the program ends is killed, so a failed test leaves nothing behind.

Errors are `"{path}: not found"`, `permission denied`, `is a directory`, `not a directory`, `already exists`, `directory not empty`, `invalid UTF-8`, `invalid path`, `byte out of range`, `not a symlink`, `same file`, `mode out of range` or `os error N`. Deployed service endpoints may perform `time` and `env` but not `fs`, `io`, `proc` or `net`; HTTP is on the [Networking](network.md) page.

## On Windows

The calls behave the same, with the differences the OS forces:

- Mode bits: Windows only has a read-only flag. `set_mode` sets it when the owner write bit (`0o200`) is clear and clears it otherwise; `file_mode` reports `0o644` or `0o444` for files and `0o755` or `0o555` for directories. `copy_file` copies contents but not the read-only flag.
- `remove_file` removes read-only files, as it does on POSIX.
- Opening a directory as a file (`read_file`, `write_file`, `copy_file`, `open_file` and friends) gives `is a directory`, which Windows itself reports as access denied.
- `run_cmd` has no signals: the status is the process exit code, never `128 + signal`. `p.stop()` terminates the process at once, without a graceful SIGTERM. There is no `/bin/sh`; run `cmd /c` or a `sh` on `PATH` (Git for Windows ships one).
- Temporary files: Windows sets `TEMP` and `TMP` rather than `TMPDIR`.
- `env_var("OS")` is `Windows_NT` on every Windows system, which is the way to tell the platforms apart.
