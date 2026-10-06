//! What differs between Unix and Windows: the shell a command runs in,
//! keeping each server's process tree together, stopping it, and opening
//! URLs.
//!
//! Each server's processes are a {@link Tree}. On Unix that's a process
//! group (the server starts in a session of its own), which the guardian
//! process also signals if blueprint dies. On Windows it's a job object the
//! server joins as soon as it starts, which the system kills when blueprint
//! exits however it exits.

import { spawn, spawnSync, type ChildProcess, type SpawnOptions } from "node:child_process"
import * as guardian from "./guardian"
import { translate } from "./wincmd"

export const isWindows = process.platform === "win32"

/** How to run `command`: `sh -c` on Unix, `cmd.exe` (after translating it) on Windows. */
export function shell(command: string): { file: string; args: string[]; options: SpawnOptions } {
  if (!isWindows) {
    // A session of its own: the server's process group is its pid, and it
    // can't touch blueprint's terminal.
    return { file: "sh", args: ["-c", command], options: { detached: true } }
  }
  const t = translate(command)
  if (!t.ok) throw new Error(t.error)
  // Passed raw: `/s` strips the outer quotes and leaves the command exactly
  // as written. `detached` gives it a console of its own, hidden.
  return {
    file: process.env.ComSpec || "cmd.exe",
    args: ["/d", "/s", "/c", `"${t.command}"`],
    options: { detached: true, windowsHide: true, windowsVerbatimArguments: true },
  }
}

/** A server's processes, which are stopped together. */
export interface Tree {
  /** Whether any process of the tree is left. */
  alive(): boolean
  /** Ask the tree to exit. */
  terminate(): void
  /** Kill the tree outright. */
  kill(): void
  /** Stop tracking the tree, once it's gone. */
  release(): void
}

/** Track the child's tree, and make sure it dies with blueprint. */
export function adopt(child: ChildProcess): Tree {
  const pid = child.pid
  if (pid === undefined) throw new Error("the process didn't start")
  return isWindows ? jobTree(pid, child) : groupTree(pid)
}

function groupTree(pgid: number): Tree {
  guardian.watch(pgid)
  const signal = (sig: NodeJS.Signals) => {
    try {
      process.kill(-pgid, sig)
    } catch {}
  }
  return {
    alive: () => groupAlive(pgid),
    terminate: () => signal("SIGTERM"),
    kill: () => signal("SIGKILL"),
    release: () => guardian.unwatch(pgid),
  }
}

export function groupAlive(pgid: number): boolean {
  try {
    process.kill(-pgid, 0)
    return true
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "EPERM"
  }
}

// ---------------------------------------------------------------------------
// Windows job objects

const JobObjectBasicAccountingInformation = 1
const JobObjectExtendedLimitInformation = 9
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
const PROCESS_SET_QUOTA = 0x0100
const PROCESS_TERMINATE = 0x0001
/** sizeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION) and the offset of LimitFlags, on x64. */
const EXTENDED_LIMIT_SIZE = 144
const LIMIT_FLAGS_OFFSET = 16
/** sizeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION) and the offset of ActiveProcesses. */
const ACCOUNTING_SIZE = 48
const ACTIVE_PROCESSES_OFFSET = 40

type Kernel32 = {
  CreateJobObjectW(attrs: null, name: null): number | null
  SetInformationJobObject(job: number, cls: number, info: Uint8Array, len: number): number
  QueryInformationJobObject(job: number, cls: number, info: Uint8Array, len: number, ret: null): number
  AssignProcessToJobObject(job: number, process: number): number
  TerminateJobObject(job: number, code: number): number
  OpenProcess(access: number, inherit: number, pid: number): number | null
  CloseHandle(handle: number): number
}

let kernel32: Kernel32 | undefined

async function loadKernel32(): Promise<Kernel32> {
  const { dlopen, FFIType: T } = await import("bun:ffi")
  const lib = dlopen("kernel32.dll", {
    CreateJobObjectW: { args: [T.ptr, T.ptr], returns: T.ptr },
    SetInformationJobObject: { args: [T.ptr, T.i32, T.ptr, T.u32], returns: T.i32 },
    QueryInformationJobObject: { args: [T.ptr, T.i32, T.ptr, T.u32, T.ptr], returns: T.i32 },
    AssignProcessToJobObject: { args: [T.ptr, T.ptr], returns: T.i32 },
    TerminateJobObject: { args: [T.ptr, T.u32], returns: T.i32 },
    OpenProcess: { args: [T.u32, T.i32, T.u32], returns: T.ptr },
    CloseHandle: { args: [T.ptr], returns: T.i32 },
  })
  return lib.symbols as unknown as Kernel32
}

/** Load the job object API. Called once at startup on Windows. */
export async function init(): Promise<void> {
  if (isWindows && !kernel32) kernel32 = await loadKernel32()
}

/**
 * Put the child in a job of its own. It joins as soon as it has started,
 * before its shell has had a chance to start anything.
 */
function jobTree(pid: number, child: ChildProcess): Tree {
  const k = kernel32
  if (!k) return taskkillTree(pid, child)
  const job = k.CreateJobObjectW(null, null)
  if (!job) throw new Error("couldn't create a job object")
  const fail = (what: string): never => {
    k.TerminateJobObject(job, 1)
    k.CloseHandle(job)
    throw new Error(what)
  }
  const limits = new Uint8Array(EXTENDED_LIMIT_SIZE)
  new DataView(limits.buffer).setUint32(LIMIT_FLAGS_OFFSET, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, true)
  if (!k.SetInformationJobObject(job, JobObjectExtendedLimitInformation, limits, EXTENDED_LIMIT_SIZE)) {
    fail("couldn't configure the job object")
  }
  const handle = k.OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid)
  if (!handle) fail("couldn't open the server's process")
  const joined = k.AssignProcessToJobObject(job, handle!)
  k.CloseHandle(handle!)
  if (!joined) fail("couldn't add the server to its job object")
  let open = true
  return {
    alive() {
      if (!open) return false
      const info = new Uint8Array(ACCOUNTING_SIZE)
      const ok = k.QueryInformationJobObject(job, JobObjectBasicAccountingInformation, info, ACCOUNTING_SIZE, null)
      // If the job can't be asked, don't wait on it forever.
      return ok !== 0 && new DataView(info.buffer).getUint32(ACTIVE_PROCESSES_OFFSET, true) > 0
    },
    // Console programs in a hidden console can't be asked to close politely,
    // so stopping kills the tree.
    terminate() {
      if (open) k.TerminateJobObject(job, 1)
    },
    kill() {
      if (open) k.TerminateJobObject(job, 1)
    },
    release() {
      if (open) k.CloseHandle(job)
      open = false
    },
  }
}

/** Without job objects, `taskkill /T` stops the tree while its shell lives. */
function taskkillTree(pid: number, child: ChildProcess): Tree {
  const kill = () => {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore", windowsHide: true })
  }
  return {
    alive: () => child.exitCode === null && child.signalCode === null,
    terminate: kill,
    kill,
    release() {},
  }
}

// ---------------------------------------------------------------------------

/** Open `url` in the default browser. */
export function openUrl(url: string): void {
  const [file, args] = isWindows
    ? ["cmd.exe", ["/d", "/c", "start", '""', url]]
    : process.platform === "darwin"
      ? ["open", [url]]
      : ["xdg-open", [url]]
  try {
    const child = spawn(file, args, { stdio: "ignore", detached: true, windowsHide: true })
    child.on("error", () => {})
    child.unref()
  } catch {}
}
