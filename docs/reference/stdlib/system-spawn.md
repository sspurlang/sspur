| `proc.spawn(prog, args, env)` | `Res[Proc, Str] ! proc`: starts `prog` (searched in `PATH`) with the extra environment pairs `env`, without waiting for it; stdin is empty and stdout and stderr are read line by line |
| `p.line(timeout_ms)` | `Opt[Str] ! proc`: the next line `p` printed, or `none` at the end of its output or after `timeout_ms` |
| `p.running()` | `Bool ! proc`: whether `p` has not exited yet |
| `p.stop()`, `p.wait()` | `Int ! proc`: the exit status; `stop` sends SIGTERM and, after 10 seconds, SIGKILL (Windows terminates the process at once), `wait` waits. `p.pid` is the process id |

A spawned process that is still running when the program ends is killed, so a failed test leaves nothing behind.
