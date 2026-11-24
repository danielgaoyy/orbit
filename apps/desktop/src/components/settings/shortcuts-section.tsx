import { useMemo, useState } from "react";
import { Search } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isMacDesktop } from "@/lib/orbit-ipc";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { useOrbitSelector } from "@/state/orbit-store";

interface ShortcutItem {
  group: string;
  action: string;
  keys: string;
}

export function ShortcutsSection() {
  const { locale, t } = useI18n();
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const [query, setQuery] = useState("");
  const isMac = isMacDesktop();
  const mod = modSymbol();
  const shift = isMac ? "⇧" : "Shift+";
  const alt = isMac ? "⌥" : "Alt+";
  const ctrl = isMac ? "⌃" : "Ctrl+";
  const backspace = isMac ? "⌫" : "Backspace";

  const groups = useMemo(() => {
    const navGroup = t("settings.groupNavigation");
    const panelsGroup = t("settings.groupPanels");
    const composerGroup = t("settings.groupComposer");
    const approvalsGroup = t("settings.groupApprovals");

    const all: ShortcutItem[] = [
      { group: navGroup, action: t("settings.scCommandPalette"), keys: `${mod}K` },
      { group: navGroup, action: t("settings.scFilterConversations"), keys: `${mod}F` },
      { group: navGroup, action: t("settings.scSettings"), keys: `${mod},` },
      { group: navGroup, action: t("settings.scNewConversation"), keys: `${mod}N` },
      { group: navGroup, action: t("settings.scNextApproval"), keys: `${mod}${shift}A` },
      { group: navGroup, action: t("settings.scSwitchConversation"), keys: `${mod}1 .. ${mod}9` },

      { group: panelsGroup, action: t("settings.scToggleSidebar"), keys: `${mod}B` },
      { group: panelsGroup, action: t("settings.scToggleSidePanel"), keys: `${mod}\\` },
      { group: panelsGroup, action: t("settings.scNewSideTab"), keys: `${mod}T` },
      { group: panelsGroup, action: t("settings.scToggleOverview"), keys: `${mod}${alt}O` },
      { group: panelsGroup, action: t("settings.scQuickOpenFile"), keys: `${mod}P` },
      { group: panelsGroup, action: t("settings.scToggleDiff"), keys: `${mod}D` },
      { group: panelsGroup, action: t("settings.scToggleTerminal"), keys: `${mod}J / ${ctrl}\`` },
      { group: panelsGroup, action: t("settings.scSideQuestion"), keys: `${mod};` },
      { group: panelsGroup, action: t("settings.scExternalEditor"), keys: `${mod}O` },

      {
        group: composerGroup,
        action: t("settings.scSendPrompt"),
        keys: enterToSend ? "Enter" : `${mod}Enter`,
      },
      {
        group: composerGroup,
        action: t("settings.scNewLine"),
        keys: enterToSend ? `${shift}Enter` : "Enter",
      },
      { group: composerGroup, action: t("settings.scCyclePermissions"), keys: `${shift}Tab` },
      { group: composerGroup, action: t("settings.scSteerAgent"), keys: `${mod}Enter` },
      { group: composerGroup, action: t("settings.scStopGeneration"), keys: "Esc Esc" },

      { group: approvalsGroup, action: t("settings.scAllow"), keys: `${mod}Y` },
      { group: approvalsGroup, action: t("settings.scAlwaysAllow"), keys: `${mod}${shift}Y` },
      { group: approvalsGroup, action: t("settings.scDeny"), keys: `${mod}${backspace}` },
      { group: approvalsGroup, action: t("settings.scSwitchApproval"), keys: `${mod}[ / ${mod}]` },
      { group: approvalsGroup, action: t("settings.scQuickSelectOption"), keys: "1 .. 9" },
    ];

    const q = query.trim().toLowerCase();
    const filtered = q
      ? all.filter(
          (b) =>
            b.action.toLowerCase().includes(q) ||
            b.group.toLowerCase().includes(q) ||
            b.keys.toLowerCase().includes(q),
        )
      : all;

    const map = new Map<string, ShortcutItem[]>();
    for (const item of filtered) {
      const list = map.get(item.group) ?? [];
      list.push(item);
      map.set(item.group, list);
    }
    return Array.from(map.entries());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale, mod, shift, alt, ctrl, backspace, query, enterToSend]);

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title={t("settings.shortcutsTitle")}
        description={t("settings.shortcutsDesc")}
      />

      <div className="relative">
        <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-3 h-[14px] w-[14px] -translate-y-1/2" />
        <Input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("settings.searchShortcutsPlaceholder")}
          className="h-[34px] rounded-[10px] pl-8.5 text-[13px]"
        />
      </div>

      {groups.length === 0 ? (
        <div className="text-muted-foreground py-8 text-center text-[13px]">
          {t("settings.noMatchingShortcuts")}
        </div>
      ) : (
        <div className="space-y-5">
          {groups.map(([groupName, items]) => (
            <div key={groupName} className="space-y-2">
              <SettingGroupLabel>{groupName}</SettingGroupLabel>
              <SettingCard>
                {items.map((item) => (
                  <div
                    key={item.action}
                    className="flex items-center justify-between gap-4 px-4 py-2.5"
                  >
                    <span className="text-foreground text-[13px]">{item.action}</span>
                    <ShortcutKbd keys={item.keys} className="shrink-0" />
                  </div>
                ))}
              </SettingCard>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
