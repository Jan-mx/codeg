"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Reorder } from "motion/react"
import { SquarePen } from "lucide-react"
import { useTranslations } from "next-intl"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useTabActions, useTabStore } from "@/contexts/tab-context"
import type { TabItem as TabItemData } from "@/contexts/tab-context"
import { useWorkspaceView } from "@/contexts/workspace-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { useIsCoarsePointer } from "@/hooks/use-is-coarse-pointer"
import { useShortcutSettings } from "@/hooks/use-shortcut-settings"
import { matchShortcutEvent } from "@/lib/keyboard-shortcuts"
import { TabItem } from "./tab-item"
import { cn } from "@/lib/utils"

export function TabBar({ embedded = false }: { embedded?: boolean } = {}) {
  const t = useTranslations("Folder.conversationCard")
  const tabs = useTabStore((s) => s.tabs)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const isTileMode = useTabStore((s) => s.isTileMode)
  const {
    switchTab,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    pinTab,
    toggleTileMode,
    reorderTabs,
    openNewConversationTab,
    openChatModeTab,
  } = useTabActions()
  const allFolders = useAppWorkspaceStore((s) => s.allFolders)
  const branches = useAppWorkspaceStore((s) => s.branches)
  const { activeFolder } = useActiveFolder()
  const { openConversations } = useWorkbenchRoute()
  const { mode, activePane, filesMaximized } = useWorkspaceView()

  // New-conversation affordance at the end of the tab strip. Mirrors the
  // sidebar's "New chat": return to the conversation workspace, then open a
  // draft in the active folder — or a folderless chat when nothing is open, so
  // the button is never a dead end.
  const handleNewConversation = useCallback(() => {
    openConversations()
    if (!activeFolder) {
      openChatModeTab()
      return
    }
    openNewConversationTab(activeFolder.id, activeFolder.path)
  }, [activeFolder, openChatModeTab, openConversations, openNewConversationTab])

  const folderIndex = useMemo(() => {
    const map = new Map<number, { name: string }>()
    for (const f of allFolders) map.set(f.id, { name: f.name })
    return map
  }, [allFolders])

  const { shortcuts } = useShortcutSettings()
  const scrollRef = useRef<HTMLDivElement>(null)
  const isCoarsePointer = useIsCoarsePointer()
  const [isHovered, setIsHovered] = useState(false)
  const [touchSortingTabId, setTouchSortingTabId] = useState<string | null>(
    null
  )

  const handleWheel = useCallback((e: React.WheelEvent<HTMLDivElement>) => {
    if (e.deltaY !== 0 && scrollRef.current) {
      e.preventDefault()
      scrollRef.current.scrollLeft += e.deltaY
    }
  }, [])

  useEffect(() => {
    if (!activeTabId || !scrollRef.current) return
    const el = scrollRef.current.querySelector(`[data-tab-id="${activeTabId}"]`)
    el?.scrollIntoView({ block: "nearest", inline: "nearest" })
  }, [activeTabId])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const shouldHandleShortcut =
        mode === "conversation" ||
        (mode === "fusion" && activePane === "conversation" && !filesMaximized)
      if (!shouldHandleShortcut) return
      const isNextTab = matchShortcutEvent(event, shortcuts.next_tab)
      const isPrevTab = matchShortcutEvent(event, shortcuts.prev_tab)
      if (isNextTab || isPrevTab) {
        if (tabs.length < 2 || !activeTabId) return
        const currentIndex = tabs.findIndex((tab) => tab.id === activeTabId)
        if (currentIndex === -1) return

        event.preventDefault()
        const offset = isNextTab ? 1 : -1
        const nextIndex = (currentIndex + offset + tabs.length) % tabs.length
        switchTab(tabs[nextIndex].id)
        return
      }

      if (!matchShortcutEvent(event, shortcuts.close_current_tab)) return
      if (!activeTabId) return

      event.preventDefault()
      closeTab(activeTabId)
    }

    window.addEventListener("keydown", onKeyDown)
    return () => {
      window.removeEventListener("keydown", onKeyDown)
    }
  }, [
    activePane,
    activeTabId,
    closeTab,
    filesMaximized,
    mode,
    shortcuts.close_current_tab,
    shortcuts.next_tab,
    shortcuts.prev_tab,
    switchTab,
    tabs,
  ])

  const handleReorder = useCallback(
    (nextTabs: TabItemData[]) => {
      if (isCoarsePointer && !touchSortingTabId) return
      reorderTabs(nextTabs)
    },
    [isCoarsePointer, reorderTabs, touchSortingTabId]
  )

  const handleTouchSortingEnd = useCallback(
    () => setTouchSortingTabId(null),
    []
  )

  if (tabs.length === 0) return null

  const activeIndex = tabs.findIndex((tab) => tab.id === activeTabId)
  // When the LAST tab is active, the trailing new-conversation wrapper is its
  // right neighbour — it needs the same baseline inset a tab neighbour gets
  // (`data-adjacent-active`), so the active tab's right reverse-corner foot
  // doesn't leave a stray line poking out from under it (globals.css).
  const lastTabActive = activeIndex >= 0 && activeIndex === tabs.length - 1

  const group = (
    <Reorder.Group
      as="div"
      ref={scrollRef}
      role="tablist"
      axis="x"
      values={tabs}
      onReorder={handleReorder}
      // Embedded tabs shrink to fit (no overflow), so wheel-to-scroll is both
      // unnecessary and wrong — `overflow-hidden` still scrolls programmatically.
      onWheel={embedded ? undefined : handleWheel}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className={cn(
        // Horizontal padding is set per-branch below (not shared here), because
        // the two modes need different gutters.
        "pt-1.5 flex items-stretch",
        // Embedded in the title bar: fill its height, no scrollbar — the tabs
        // shrink browser-style to share the row (see TabItem `embedded`), sit
        // flush (`gap-0`) so their hairline separators read as dividers, and the
        // strip owns no bottom border. No bottom padding, so the tabs reach the
        // strip's bottom edge and the active (white) one merges into the detail
        // header below. It fills the row (`flex-1`) and hosts the trailing
        // new-conversation button + drag spacer as its own last children, so the
        // tabs, button, and spacer all size in ONE flex line: the tabs keep their
        // equal `basis-48` width until the row fills, then shrink together, and the
        // button always hugs the last tab. (A nested content-sized group instead
        // let the engine resolve its `flex-basis:auto` to min-content — starving
        // the tabs to their label width and detaching the button from them.)
        // `pl-2` only (NOT `px-2`): the first tab keeps its 0.5rem left gutter for
        // the first-child seam-patch, but there is NO right padding — so the trailing
        // wrapper's `ws-strip-line` reaches the group's right edge and the bottom
        // hairline stays continuous into the right reserve. (A right `px-2` left an
        // 8px hole in the hairline there, since the wrapper — not a tab — is the
        // group's last child and gets no last-child seam-patch.)
        // Standalone (mobile panel row): keep the h-10 row + border + horizontal
        // scroll with a hover scrollbar and the original inter-tab gap.
        embedded
          ? "h-full min-w-0 flex-1 gap-0 overflow-hidden pl-2"
          : [
              "h-10 gap-1.5 px-1.5 border-b border-border overflow-x-scroll",
              isHovered
                ? [
                    "pb-0.5",
                    "[&::-webkit-scrollbar]:h-1",
                    "[&::-webkit-scrollbar-track]:bg-transparent",
                    "[&::-webkit-scrollbar-thumb]:rounded-full",
                    "[&::-webkit-scrollbar-thumb]:bg-border",
                  ]
                : ["pb-1.5", "[&::-webkit-scrollbar]:h-0"],
            ]
      )}
    >
      {tabs.map((tab, index) => {
        const folderInfo = folderIndex.get(tab.folderId)
        // Neighbours of the active tab inset their workspace-bg baseline so the
        // active tab's transparent reverse-corner foot (which flares over them)
        // doesn't leave a stray line under it (globals.css `data-adjacent-active`).
        const adjacentActive =
          activeIndex < 0
            ? undefined
            : index === activeIndex - 1
              ? "before"
              : index === activeIndex + 1
                ? "after"
                : undefined
        return (
          <TabItem
            key={tab.id}
            tab={tab}
            isActive={tab.id === activeTabId}
            isTileMode={isTileMode}
            embedded={embedded}
            adjacentActive={adjacentActive}
            folderName={folderInfo?.name ?? null}
            folderBranch={branches.get(tab.folderId) ?? null}
            onSwitch={switchTab}
            onClose={closeTab}
            onCloseOthers={closeOtherTabs}
            onCloseAll={closeAllTabs}
            onPin={pinTab}
            onToggleTile={toggleTileMode}
            isCoarsePointer={isCoarsePointer}
            isTouchSorting={touchSortingTabId === tab.id}
            onTouchSortingStart={setTouchSortingTabId}
            onTouchSortingEnd={handleTouchSortingEnd}
          />
        )
      })}
      {/* Title-bar strip only: the new-conversation button + drag spacer are the
          Reorder.Group's own trailing children, so they share the tabs' flex line
          — the button hugs the last tab and the spacer fills the leftover row as a
          window-drag region. They are not Reorder.Items, so dragging a tab only
          ever permutes the tabs (verified: reordering is unaffected). Wrapped in
          one `flex-1` `ws-strip-line` box so the workspace-bg bottom hairline runs
          unbroken under both — the short `self-center h-7` button can't carry the
          line itself. NO `min-w-0`: its min-content (the shrink-0 button + the
          spacer's `min-w-10`) is its floor, so under many-tab overflow the tabs
          shrink to reserve it instead of it collapsing to 0 and clipping the
          button. Off (no bg image): ws-strip-line is inert. */}
      {embedded && (
        <div
          // `relative` anchors the `data-adjacent-active` inset baseline
          // (globals.css `::after`) used when the last tab is active.
          data-adjacent-active={lastTabActive ? "after" : undefined}
          className="relative flex h-full flex-1 items-stretch ws-strip-line"
        >
          <button
            type="button"
            onClick={handleNewConversation}
            // Ghost-style CIRCULAR icon button, evenly inset from the strip's
            // three visible edges so its round hover fill never touches the last
            // tab. `self-start` seats it against the group's `pt-1.5` top rather
            // than centering in the pt-shortened trailing box (which pushed it
            // 3px from the bottom, 9px from the top): with `h-7` on the `h-10`
            // strip that yields an equal 6px top and 6px bottom gap, so its center
            // still lands on the strip midline (matching the tab content).
            // `ml-1.5` adds a matching 6px LEFT gap from the last tab's edge — so
            // on hover the circle reads as evenly floated off the top, bottom, and
            // left lines. The hover fill reuses the changed-files reply card's
            // collapsed-header tint (`bg-accent/40`, see reply-artifacts): a
            // translucent accent that reads as a light frost over a workspace
            // background image, instead of the opaque dark patch `bg-foreground/10`
            // showed there.
            className="ml-1.5 mr-0.5 flex h-7 w-7 shrink-0 items-center justify-center self-start rounded-full text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
            aria-label={t("newConversation")}
            title={t("newConversation")}
          >
            <SquarePen className="h-3.5 w-3.5" />
          </button>
          {/* Drag spacer, floored at `min-w-10` (40px) instead of `min-w-0`: even
              when many tabs overflow and squeeze this region, a grabbable
              window-drag gap always remains to the RIGHT of the new-conversation
              button, so the button never reaches the strip's right edge and the
              packed title bar stays draggable. */}
          <div data-tauri-drag-region className="h-full min-w-10 flex-1" />
        </div>
      )}
    </Reorder.Group>
  )

  return group
}
