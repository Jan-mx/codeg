import { describe, expect, it } from "vitest"

import { resolveAuxTabView } from "./aux-panel"

describe("resolveAuxTabView", () => {
  it("shows all tabs and keeps the selection in a folder workspace", () => {
    expect(resolveAuxTabView("file_tree", 1, false)).toEqual({
      showFolderTabs: true,
      effectiveTab: "file_tree",
    })
    expect(resolveAuxTabView("session_details", 1, false)).toEqual({
      showFolderTabs: true,
      effectiveTab: "session_details",
    })
  })

  it("collapses to Session Details in chat mode, even with a bound folder", () => {
    // A bound chat conversation has a (hidden) folder id but is chat mode.
    expect(resolveAuxTabView("git_log", 1, true)).toEqual({
      showFolderTabs: false,
      effectiveTab: "session_details",
    })
  })

  it("collapses to Session Details when no folder is open", () => {
    expect(resolveAuxTabView("changes", null, false)).toEqual({
      showFolderTabs: false,
      effectiveTab: "session_details",
    })
  })

  it("overrides a stale folder-tab selection with a valid shown tab", () => {
    // Stored selection is a folder tab but folder tabs are hidden: the shown
    // tab must fall back so Radix never points at a triggerless value.
    expect(resolveAuxTabView("file_tree", null, false).effectiveTab).toBe(
      "session_details"
    )
  })
})
