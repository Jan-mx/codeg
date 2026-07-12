import { describe, expect, it } from "vitest"

import { MONACO_UNICODE_HIGHLIGHT_OPTIONS } from "./monaco-themes"

// Regression guard for issue #329: Monaco boxed ordinary CJK full-width
// punctuation (`：` `；` `，` `！` `？` `（` `）` …) because its unicode-highlight
// feature flags characters that look confusable with / are non-basic ASCII.
// Both mechanisms must stay disabled so Chinese/Japanese prose renders cleanly.
describe("MONACO_UNICODE_HIGHLIGHT_OPTIONS", () => {
  it("disables the mechanisms that box visible CJK punctuation", () => {
    expect(MONACO_UNICODE_HIGHLIGHT_OPTIONS.ambiguousCharacters).toBe(false)
    expect(MONACO_UNICODE_HIGHLIGHT_OPTIONS.nonBasicASCII).toBe(false)
  })

  it("keeps invisible-character highlighting at its default (still useful)", () => {
    // Intentionally untouched: surfacing zero-width / BOM characters never
    // boxes legible text and helps catch copy-paste gremlins.
    expect(MONACO_UNICODE_HIGHLIGHT_OPTIONS.invisibleCharacters).toBeUndefined()
  })
})
