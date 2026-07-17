"use client"

import { memo, useCallback, useMemo, useRef } from "react"
import { Reorder } from "motion/react"
import { X } from "lucide-react"
import { useTranslations } from "next-intl"
import { cn, handleMiddleClickClose } from "@/lib/utils"
import type { ConversationStatus } from "@/lib/types"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { useLongPressDrag } from "@/hooks/use-long-press-drag"
import type { TabItem as TabItemData } from "@/contexts/tab-context"

interface TabItemProps {
  tab: TabItemData
  isActive: boolean
  isTileMode: boolean
  /** Browser-style shrink: fill/shrink to share the row width instead of a
   *  fixed intrinsic size (title-bar embedded strips). Off = mobile scroll row. */
  embedded?: boolean
  folderName: string | null
  folderBranch: string | null
  onSwitch: (tabId: string) => void
  onClose: (tabId: string) => void
  onCloseOthers: (tabId: string) => void
  onCloseAll: () => void
  onPin: (tabId: string) => void
  onToggleTile: () => void
  isCoarsePointer: boolean
  isTouchSorting: boolean
  onTouchSortingStart: (tabId: string) => void
  onTouchSortingEnd: () => void
}

export const TabItem = memo(function TabItem({
  tab,
  isActive,
  isTileMode,
  embedded = false,
  folderName,
  folderBranch,
  onSwitch,
  onClose,
  onCloseOthers,
  onCloseAll,
  onPin,
  onToggleTile,
  isCoarsePointer,
  isTouchSorting,
  onTouchSortingStart,
  onTouchSortingEnd,
}: TabItemProps) {
  const t = useTranslations("Folder.tabs")
  const itemRef = useRef<HTMLDivElement>(null)

  const resolvedFolderName = folderName ?? String(tab.folderId)
  const tooltip = folderBranch
    ? `${resolvedFolderName} · ${folderBranch}  —  ${tab.title}`
    : `${resolvedFolderName}  —  ${tab.title}`

  const clearResidualStyles = useCallback(() => {
    const el = itemRef.current
    if (!el) return
    el.style.transform = ""
    el.style.zIndex = ""
    el.style.position = ""
    el.style.userSelect = ""
  }, [])

  const handleLongPressStart = useCallback(
    () => onTouchSortingStart(tab.id),
    [onTouchSortingStart, tab.id]
  )

  const { dragControls, gestureHandlers } = useLongPressDrag({
    enabled: isCoarsePointer,
    onStart: handleLongPressStart,
    onEnd: onTouchSortingEnd,
    onDragSettle: clearResidualStyles,
  })

  const handleClick = useCallback(() => {
    onSwitch(tab.id)
  }, [onSwitch, tab.id])

  const handleDoubleClick = useCallback(() => {
    if (!tab.isPinned) {
      onPin(tab.id)
    }
  }, [onPin, tab.id, tab.isPinned])

  const handleClose = useCallback(() => {
    onClose(tab.id)
  }, [onClose, tab.id])

  const handleCloseOthers = useCallback(() => {
    onCloseOthers(tab.id)
  }, [onCloseOthers, tab.id])

  const whileDrag = useMemo(() => ({ scale: 1.03 }), [])

  return (
    <Reorder.Item
      ref={itemRef}
      as="div"
      value={tab}
      data-tab-id={tab.id}
      drag="x"
      dragControls={dragControls}
      dragListener={!isCoarsePointer}
      whileDrag={whileDrag}
      {...gestureHandlers}
      onLayoutAnimationComplete={clearResidualStyles}
      data-tab-item
      data-active={embedded && isActive ? "true" : undefined}
      className={cn(
        "cursor-grab active:cursor-grabbing",
        // Embedded (browser-style): each tab sizes to its content (`basis-auto`)
        // up to `max-w-[15rem]`, so a few tabs sit at their natural width and the
        // new-conversation button hugs the last one — the leftover row stays a
        // window-drag region. `grow-0` keeps them from stretching to fill; they
        // still `shrink` together (down to `min-w-0`, the label truncates) once
        // the row fills. `browser-tab-item` draws the left-edge hairline
        // separator (globals.css) as a 1px divider at each shared edge; tabs sit
        // flush (no gutter) so the line is the only separation, and the inner row
        // owns its own `overflow-hidden`. The active tab is raised (`z-10`) so
        // its reverse-corner seat is never covered by a hovered neighbour's flare.
        // Standalone: rounded pill, intrinsic size + horizontal scroll (mobile).
        embedded
          ? "browser-tab-item min-w-0 grow-0 shrink basis-auto max-w-[15rem] data-[active=true]:z-10"
          : "rounded-full shrink-0",
        !isCoarsePointer &&
          (embedded
            ? "active:z-50"
            : "active:opacity-90 active:shadow-md active:z-50"),
        isTouchSorting && "z-50 opacity-90 shadow-md ring-1 ring-primary/25"
      )}
    >
      {/* Reverse (concave) bottom corners — the browser-tab seat (globals.css).
          Absolute + decorative, so it never affects layout. Rendered for every
          embedded tab; CSS reveals it when the tab is active or hovered. */}
      {embedded && <span aria-hidden className="browser-tab-seat" />}
      <ContextMenu>
        <ContextMenuTrigger asChild disabled={isTouchSorting}>
          <div
            role="tab"
            aria-selected={isActive}
            onClick={handleClick}
            onDoubleClick={handleDoubleClick}
            onMouseDown={(event) => handleMiddleClickClose(event, handleClose)}
            className={cn(
              "group/tab relative flex items-center h-full gap-1.5 text-xs",
              "cursor-pointer select-none transition-colors",
              embedded
                ? [
                    // Browser-style tab: seats against the sidebar-shaded strip
                    // with a white (bg-background) active fill and a rounded top,
                    // reaching the strip's bottom so it merges into the white
                    // detail header below. `overflow-hidden` clips the shrunken
                    // row so a tight tab can't paint/click over its neighbor.
                    // `pb-1.5` balances the group's `pt-1.5` gap so the content
                    // (dot/title/close) centers on the h-10 strip's midline, not
                    // 3px low in the shorter tab box (the fill still reaches the
                    // strip bottom — padding only insets the content).
                    "w-full min-w-0 overflow-hidden rounded-t-lg px-2 pb-1.5",
                    isActive
                      ? "bg-background text-foreground"
                      : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                  ]
                : [
                    "shrink-0 rounded-full px-3 hover:bg-primary/8",
                    isActive
                      ? "bg-primary/10 text-foreground"
                      : "text-muted-foreground",
                  ]
            )}
          >
            <ConversationStatusDot
              status={tab.status as ConversationStatus | undefined}
            />
            <span
              className={cn(
                // Embedded: grow + shrink and ellipsis-truncate as the tab
                // tightens. The inline close button (below) reserves its own
                // space in the row, so the label never runs under it.
                // Standalone: ellipsis cap in the scroll row.
                embedded ? "min-w-0 flex-1 truncate" : "truncate max-w-[140px]",
                !tab.isPinned && "[font-style:oblique]"
              )}
              title={tooltip}
            >
              {tab.title}
            </span>
            <button
              type="button"
              className={cn(
                "rounded-md hover:bg-foreground/10",
                // Embedded: an in-flow (inline) icon box that reserves its own
                // space in the flex row, so it centers with the title via the
                // row's `items-center` (no transform → crisp on WebKit, unlike a
                // translated absolute overlay) and the label truncates before it.
                // It stays laid out even while hidden (opacity-0) so revealing it
                // on hover never shifts the tab width; pointer events are gated
                // off while hidden so it can't eat clicks. Standalone: an in-flow
                // chip in the scroll row.
                embedded
                  ? "flex h-4 w-4 shrink-0 items-center justify-center"
                  : "shrink-0 p-0.5",
                isActive
                  ? "opacity-100"
                  : embedded
                    ? "opacity-0 pointer-events-none group-hover/tab:opacity-100 group-hover/tab:pointer-events-auto"
                    : "opacity-0 group-hover/tab:opacity-100"
              )}
              onClick={(event) => {
                event.stopPropagation()
                handleClose()
              }}
              aria-label={t("closeConversationTab")}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem onSelect={handleClose}>{t("close")}</ContextMenuItem>
          <ContextMenuItem onSelect={handleCloseOthers}>
            {t("closeOthers")}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={onToggleTile}>
            {isTileMode ? t("untileDisplay") : t("tileDisplay")}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={onCloseAll}>
            {t("closeAll")}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </Reorder.Item>
  )
})
