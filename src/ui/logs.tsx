//! The log console: the selected project's retained lines, drawn a screen at
//! a time so thousands of lines stay fast, with a filter.

import { TextAttributes } from "@opentui/core"
import { parse, strip } from "../core/ansi"
import { MAX_LOG_LINES } from "../core/manager"
import type { Board } from "./board"
import { ansiColor, theme } from "./theme"

export function Logs({ board }: { board: Board }) {
  const e = board.entry
  const index = board.index
  const shown = index.lines.length
  const total = e?.logs.length ?? 0
  let count = ""
  if (e && total > 0) {
    count = index.filtering() ? `${shown} of ${total} lines` : `${total} lines`
    if (e.logStart > 0) count += ` · keeps the last ${MAX_LOG_LINES}`
  }
  const filterRow = board.mode === "filter" || board.filter !== ""
  const rows: string[] = []
  if (e) for (let i = board.top; i < Math.min(shown, board.top + board.viewRows); i++) rows.push(index.line(e, i) ?? "")

  let hint = ""
  if (e && shown === 0) {
    hint = index.filtering() ? "No lines match the filter." : e.run ? "Waiting for output…" : "Not running. Press enter to start it."
  }

  return (
    <box
      flexGrow={1}
      minHeight={3}
      border
      borderStyle="rounded"
      borderColor={board.mode === "filter" ? theme.accent : theme.border}
      title={" Logs "}
      titleAlignment="left"
      bottomTitle={count ? ` ${count}${board.follow || !shown ? "" : " · end ↓ latest"} ` : undefined}
      bottomTitleAlignment="right"
      flexDirection="column"
      paddingLeft={1}
      paddingRight={1}
      onMouseScroll={(ev) => {
        const dir = ev.scroll?.direction
        if (dir === "up") board.scrollLogs(-3)
        else if (dir === "down") board.scrollLogs(3)
      }}
    >
      {filterRow && (
        <box height={1} flexDirection="row">
          <text fg={board.mode === "filter" ? theme.accent : theme.muted}>{"/ "}</text>
          {board.mode === "filter" ? (
            <input
              flexGrow={1}
              value={board.filter}
              focused
              placeholder="filter logs"
              textColor={theme.text}
              placeholderColor={theme.faint}
              backgroundColor={theme.bg}
              focusedBackgroundColor={theme.bg}
              cursorColor={theme.accent}
              onInput={(v) => board.setFilter(v)}
            />
          ) : (
            <text wrapMode="none">
              <span fg={theme.text}>{board.filter}</span>
              <span fg={theme.faint}>{"   / edit · esc clear"}</span>
            </text>
          )}
        </box>
      )}
      {hint && <text fg={theme.muted}>{hint}</text>}
      {rows.map((raw, i) => (
        <LogLine key={board.top + i} raw={raw} query={index.query} />
      ))}
    </box>
  )
}

/**
 * One log line. Status lines (`── ... ──`) are drawn in the accent and the
 * command line faint; others keep their colors, or, while filtering, show
 * plain with the matches marked.
 */
function LogLine({ raw, query }: { raw: string; query: string }) {
  if (!raw) return <text> </text>
  if (query) {
    const text = strip(raw)
    const lower = text.toLowerCase()
    const parts: { text: string; match: boolean }[] = []
    // Lowercasing can change lengths outside ASCII; only mark matches when offsets line up.
    if (lower.length === text.length) {
      let at = 0
      for (let i = lower.indexOf(query); i >= 0; i = lower.indexOf(query, at)) {
        if (i > at) parts.push({ text: text.slice(at, i), match: false })
        parts.push({ text: text.slice(i, i + query.length), match: true })
        at = i + query.length
      }
      if (at < text.length) parts.push({ text: text.slice(at), match: false })
    } else {
      parts.push({ text, match: false })
    }
    return (
      <text wrapMode="none">
        {parts.map((p, i) =>
          p.match ? (
            <span key={i} fg={theme.onAccent} bg={theme.accent}>
              {p.text}
            </span>
          ) : (
            <span key={i} fg={theme.text}>
              {p.text}
            </span>
          ),
        )}
      </text>
    )
  }
  if (raw.startsWith("── ")) {
    return (
      <text wrapMode="none" fg={theme.accent}>
        {raw}
      </text>
    )
  }
  if (raw.startsWith("$ PORT=")) {
    return (
      <text wrapMode="none" fg={theme.faint}>
        {raw}
      </text>
    )
  }
  const runs = parse(raw)
  return (
    <text wrapMode="none">
      {runs.map((r, i) => {
        let attributes = TextAttributes.NONE
        if (r.style.bold) attributes |= TextAttributes.BOLD
        if (r.style.dim) attributes |= TextAttributes.DIM
        if (r.style.italic) attributes |= TextAttributes.ITALIC
        if (r.style.underline) attributes |= TextAttributes.UNDERLINE
        return (
          <span key={i} fg={r.style.fg ? ansiColor(r.style.fg) : theme.text} attributes={attributes}>
            {r.text || " "}
          </span>
        )
      })}
    </text>
  )
}
