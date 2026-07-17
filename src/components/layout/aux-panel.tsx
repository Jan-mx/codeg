"use client"

import { useCallback, useEffect, useState } from "react"
import { Folder, FolderPen, GitCommit, ReceiptText } from "lucide-react"
import { useTranslations } from "next-intl"
import {
  useAuxPanelContext,
  type AuxPanelTab,
} from "@/contexts/aux-panel-context"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useIsActiveChatMode } from "@/hooks/use-is-active-chat-mode"
import { useIsMobile } from "@/hooks/use-mobile"
import { cn } from "@/lib/utils"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { SessionDetailsTab } from "./aux-panel-session-details-tab"
import { FileTreeTab } from "./aux-panel-file-tree-tab"
import { GitChangesTab } from "./aux-panel-git-changes-tab"
import { GitLogTab } from "./aux-panel-git-log-tab"

const LAZY_TABS: AuxPanelTab[] = ["file_tree", "changes", "git_log"]

/**
 * Decide which aux-panel tabs are available and which to actually show.
 *
 * The folder-scoped tabs (files/changes/commits) only make sense with a real
 * folder workspace open, so chat sessions and the folderless state collapse to
 * just the Session Details tab. `effectiveTab` keeps the rendered selection
 * valid even when the stored `activeTab` is a now-hidden folder tab, avoiding a
 * one-frame flash before the reconciling effect corrects the stored value.
 */
export function resolveAuxTabView(
  activeTab: AuxPanelTab,
  activeFolderId: number | null,
  isChatMode: boolean
): { showFolderTabs: boolean; effectiveTab: AuxPanelTab } {
  const showFolderTabs = activeFolderId != null && !isChatMode
  return {
    showFolderTabs,
    effectiveTab: showFolderTabs ? activeTab : "session_details",
  }
}

export function AuxPanel() {
  const t = useTranslations("Folder.auxPanel.tabs")
  const tDetails = useTranslations("Folder.sessionDetails")
  const { isOpen, activeTab, setActiveTab } = useAuxPanelContext()
  const { activeFolderId } = useActiveFolder()
  const isChatMode = useIsActiveChatMode()
  const isMobile = useIsMobile()
  const [mountedTabs, setMountedTabs] = useState<Set<AuxPanelTab>>(
    () => new Set(LAZY_TABS.filter((tab) => tab === activeTab))
  )

  const { showFolderTabs, effectiveTab } = resolveAuxTabView(
    activeTab,
    activeFolderId,
    isChatMode
  )

  // Ensure the shown tab is mounted (covers both user clicks and programmatic changes)
  if (
    isOpen &&
    LAZY_TABS.includes(effectiveTab) &&
    !mountedTabs.has(effectiveTab)
  ) {
    setMountedTabs((prev) => new Set(prev).add(effectiveTab))
  }

  // Reconcile the stored selection when folder tabs disappear (e.g. entering a
  // chat session), so other consumers of `activeTab` stay in sync with what's
  // shown. Done in an effect — never a render-time setState on the provider.
  useEffect(() => {
    if (!showFolderTabs && activeTab !== "session_details") {
      setActiveTab("session_details")
    }
  }, [showFolderTabs, activeTab, setActiveTab])

  const handleTabValueChange = useCallback(
    (value: string) => {
      setActiveTab(value as AuxPanelTab)
    },
    [setActiveTab]
  )

  // Shared across the mobile underline row and the desktop segmented control.
  // `compact` overrides the base full-height, equal-flex trigger into a short,
  // content-width pill for the segmented look; mobile keeps the base styling.
  const renderTabTriggers = (compact: boolean) => {
    const triggerClassName = compact
      ? "h-6 flex-none rounded-md px-2"
      : undefined
    return (
      <>
        <TabsTrigger
          value="session_details"
          title={tDetails("menuLabel")}
          aria-label={tDetails("menuLabel")}
          className={triggerClassName}
        >
          <ReceiptText className="h-3.5 w-3.5" />
        </TabsTrigger>
        {showFolderTabs && (
          <>
            <TabsTrigger
              value="file_tree"
              title={t("files")}
              aria-label={t("files")}
              className={triggerClassName}
            >
              <Folder className="h-3.5 w-3.5" />
            </TabsTrigger>
            <TabsTrigger
              value="changes"
              title={t("changes")}
              aria-label={t("changes")}
              className={triggerClassName}
            >
              <FolderPen className="h-3.5 w-3.5" />
            </TabsTrigger>
            <TabsTrigger
              value="git_log"
              title={t("commits")}
              aria-label={t("commits")}
              className={triggerClassName}
            >
              <GitCommit className="h-3.5 w-3.5" />
            </TabsTrigger>
          </>
        )}
      </>
    )
  }

  if (!isOpen) return null

  return (
    // Desktop: background matches the middle workspace (bg-background), not the
    // darker sidebar shade, so the right column reads as one surface with it.
    // Mobile (Sheet) is unchanged — keep the sidebar shade.
    <aside
      className={cn(
        "group/aux-panel flex h-full min-h-0 flex-col overflow-hidden text-sidebar-foreground select-none",
        isMobile ? "bg-sidebar" : "bg-background"
      )}
    >
      <Tabs
        value={effectiveTab}
        onValueChange={handleTabValueChange}
        className="flex h-full min-h-0 flex-col gap-0"
      >
        {isMobile ? (
          // Mobile (Sheet): unchanged — full-width underline tabs + a divider.
          <TabsList
            variant="line"
            className="h-10 w-full shrink-0 justify-start border-b border-border px-3 group-data-horizontal/tabs:h-10"
          >
            {renderTabTriggers(false)}
            {/* Trailing drag region lets the empty part of the tab row move
                the window. */}
            <div data-tauri-drag-region className="h-full min-w-0 flex-1" />
          </TabsList>
        ) : (
          // Desktop: a compact segmented control pinned top-LEFT of the h-10
          // strip. It shares that row with the fixed top-right window-chrome
          // overlay (terminal / aux / settings), which floats over the trailing
          // drag region — the tabs sit left, the buttons float right, so they
          // never collide. The strip is always h-10 (reserving the overlay's
          // height); when Session Details is the only tab (chat / folderless)
          // the control is `hidden` (display:none) — that drops the lone trigger
          // out of the tab order (unlike `sr-only`, which stays keyboard
          // focusable and would trap Tab on an invisible control) while the
          // TabsContent's aria-labelledby still resolves the panel's name from
          // the directly-referenced hidden trigger, so it stays labelled without
          // showing a pointless single-tab control.
          <div className="flex h-10 shrink-0 items-center gap-2 bg-muted pl-3 pr-2">
            {/* `bg-muted` matches the conversation/file strips + bottom
                StatusBar. The segmented track then needs a recessed groove
                (`bg-foreground/[0.06]`) instead of the old `bg-muted/60`, which
                would vanish against the now-muted strip; the active trigger
                (bg-background) still reads as a raised white pill. */}
            <TabsList
              variant="default"
              className={cn(
                "h-7 gap-0.5 rounded-lg bg-foreground/[0.06] p-0.5 group-data-horizontal/tabs:h-7",
                !showFolderTabs && "hidden"
              )}
            >
              {renderTabTriggers(true)}
            </TabsList>
            {/* Empty row remainder (under the floating overlay) stays a
                window-drag region. */}
            <div data-tauri-drag-region className="h-full min-w-0 flex-1" />
          </div>
        )}

        <TabsContent
          value="session_details"
          forceMount
          className="mt-0 flex-1 min-h-0 overflow-hidden"
        >
          <SessionDetailsTab />
        </TabsContent>
        <TabsContent
          value="file_tree"
          forceMount
          className="mt-0 flex-1 min-h-0 overflow-hidden"
        >
          {mountedTabs.has("file_tree") ? <FileTreeTab /> : null}
        </TabsContent>
        <TabsContent
          value="changes"
          forceMount
          className="mt-0 flex-1 min-h-0 overflow-hidden"
        >
          {mountedTabs.has("changes") ? <GitChangesTab /> : null}
        </TabsContent>
        <TabsContent
          value="git_log"
          forceMount
          className="mt-0 flex-1 min-h-0 overflow-hidden"
        >
          {mountedTabs.has("git_log") ? <GitLogTab /> : null}
        </TabsContent>
      </Tabs>
    </aside>
  )
}
