# lldb script for gen.sh: runs a program with a breakpoint on every function of its executable and
# writes the file address of each function in the order of its first call.
# lldb -o 'command script import trace.py' -o 'ftrace OUT STDIN EXE ARGS...'; FTRACE_ENV is an `env -0` dump.
import os
import shlex

import lldb


def ftrace(debugger, command, result, internal_dict):
    out, stdin, exe, *argv = shlex.split(command)
    debugger.SetAsync(False)
    debugger.HandleCommand("settings set target.skip-prologue false")
    target = debugger.CreateTarget(exe)
    mods = lldb.SBFileSpecList()
    mods.Append(lldb.SBFileSpec(exe))
    bp = target.BreakpointCreateByRegex(".", mods, lldb.SBFileSpecList())
    li = lldb.SBLaunchInfo(argv)
    li.SetWorkingDirectory(os.path.dirname(out))
    # lldb's own environment has xcrun's additions (SDKROOT and others), which change sspur's run cache key.
    env = open(os.environ["FTRACE_ENV"]).read().split("\0")
    li.SetEnvironmentEntries([e for e in env if "=" in e], False)
    li.AddOpenFileAction(0, stdin, True, False)
    li.AddOpenFileAction(1, "/dev/null", False, True)
    li.AddOpenFileAction(2, "/dev/null", False, True)
    err = lldb.SBError()
    proc = target.Launch(li, err)
    if not err.Success():
        result.SetError(str(err))
        return
    seen = []
    while proc.GetState() == lldb.eStateStopped:
        for th in proc:
            if th.GetStopReason() == lldb.eStopReasonBreakpoint:
                loc = bp.FindLocationByID(th.GetStopReasonDataAtIndex(1))
                if loc.IsValid() and loc.IsEnabled():
                    loc.SetEnabled(False)
                    seen.append(loc.GetAddress().GetFileAddress())
        proc.Continue()
    with open(out, "w") as f:
        f.writelines("%x\n" % a for a in seen)
    print("%s: %d functions, exit %d" % (os.path.basename(out), len(seen), proc.GetExitStatus()))


def __lldb_init_module(debugger, internal_dict):
    debugger.HandleCommand("command script add -f trace.ftrace ftrace")
