//! Which retained lines the console shows, by line number (see
//! {@link Entry.logStart}), kept in step with the log a change at a time.

import { strip } from "./ansi"
import type { Entry, Id } from "./manager"

export type Change =
  | { kind: "none" }
  /** Different lines altogether. */
  | { kind: "reset" }
  /** `dropped` lines left the front and `added` arrived at the back. */
  | { kind: "splice"; dropped: number; added: number }

export class LogIndex {
  /** The project and (lowercased) filter the lines were picked for. */
  private key?: { id: Id; query: string }
  /** Numbers of the lines shown, oldest first. */
  lines: number[] = []
  /** Number of the next line not yet looked at. */
  private next = 0

  sync(e: Entry | undefined, filter: string): Change {
    if (!e) {
      const had = this.key !== undefined || this.lines.length > 0
      this.key = undefined
      this.lines = []
      return { kind: had ? "reset" : "none" }
    }
    const query = filter.trim().toLowerCase()
    const end = e.logStart + e.logs.length
    const pick = (from: number) => {
      const out: number[] = []
      for (let n = from; n < end; n++) if (matches(e.logs[n - e.logStart]!, query)) out.push(n)
      return out
    }
    if (this.key?.id !== e.id || this.key.query !== query) {
      this.key = { id: e.id, query }
      this.lines = pick(e.logStart)
      this.next = end
      return { kind: "reset" }
    }
    let dropped = 0
    while (dropped < this.lines.length && this.lines[dropped]! < e.logStart) dropped++
    if (dropped) this.lines.splice(0, dropped)
    const added = pick(Math.max(this.next, e.logStart))
    for (const n of added) this.lines.push(n)
    this.next = end
    return dropped || added.length ? { kind: "splice", dropped, added: added.length } : { kind: "none" }
  }

  get query(): string {
    return this.key?.query ?? ""
  }

  filtering(): boolean {
    return this.query !== ""
  }

  /** The raw text of shown line `ix`. */
  line(e: Entry, ix: number): string | undefined {
    const n = this.lines[ix]
    return n === undefined ? undefined : e.logs[n - e.logStart]
  }

  /** The text of the lines shown, without escapes, for copying or saving. */
  plainText(e: Entry): string {
    let out = ""
    for (const n of this.lines) {
      const line = e.logs[n - e.logStart]
      if (line !== undefined) out += strip(line) + "\n"
    }
    return out
  }
}

function matches(line: string, query: string): boolean {
  return !query || strip(line).toLowerCase().includes(query)
}

/** An uptime, short: `5s`, `2m`, `1h02`. */
export function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m`
  return `${Math.floor(s / 3600)}h${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}`
}
