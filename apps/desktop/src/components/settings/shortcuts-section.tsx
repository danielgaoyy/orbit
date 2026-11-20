import { useMemo, useState } from "react";
import { Search } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { modSymbol } from "@/lib/keybindings";
import { ShortcutKbd } from "@/components/ui/overlays";
import { useOrbitSelector } from "@/state/orbit-store";

interface ShortcutItem {
  group: string;
  action: string;
  keys: string;
}

export function ShortcutsSection() {
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const [query, setQuery] = useState("");
  const mod = modSymbol();

  const groups = useMemo(() => {
    const all: ShortcutItem[] = [
      { group: "Navigation", action: "Command Palette", keys: `${mod}K` },
      { group: "Navigation", action: "Settings", keys: `${mod},` },
      { group: "Navigation", action: "New Conversation", keys: `${mod}N` },
      { group: "Navigation", action: "Next Pending Approval", keys: `${mod}⇧A` },
      { group: "Navigation", action: "Switch Conversation 1–9", keys: `${mod}1 .. ${mod}9` },

      { group: "Panels", action: "Toggle Sidebar", keys: `${mod}B` },
      { group: "Panels", action: "Toggle Right Side Panel", keys: `${mod}\\` },
      { group: "Panels", action: "Toggle Stage Overview", keys: `${mod}⌥O` },
      { group: "Panels", action: "Quick Open File", keys: `${mod}P` },
      { group: "Panels", action: "Toggle Diff Review", keys: `${mod}D` },
      { group: "Panels", action: "Toggle Terminal", keys: `${mod}J / ⌃\`` },
      { group: "Panels", action: "Side Question (/btw)", keys: `${mod};` },
      { group: "Panels", action: "Open in External Editor", keys: `${mod}O` },

      {
        group: "Composer",
        action: "Send or Queue Prompt",
        keys: enterToSend ? "Enter" : `${mod}Enter`,
      },
      {
        group: "Composer",
        action: "Insert New Line",
        keys: enterToSend ? "⇧Enter" : "Enter",
      },
      { group: "Composer", action: "Cycle Access Permissions", keys: "⇧Tab" },
      { group: "Composer", action: "Steer Running Agent", keys: "⌥Enter" },
      { group: "Composer", action: "Stop Generation", keys: "Esc Esc" },

      { group: "Approvals", action: "Allow Request", keys: `${mod}Y` },
      { group: "Approvals", action: "Always Allow in Session", keys: `${mod}⇧Y` },
      { group: "Approvals", action: "Deny Request", keys: `${mod}⌫` },
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
  }, [mod, query, enterToSend]);

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Shortcuts"
        description="Keyboard shortcuts across the workspace."
      />

      <div className="border-border bg-background/60 flex h-[34px] items-center gap-2 rounded-[10px] border px-3">
        <Search className="text-muted-foreground h-[14.5px] w-[14.5px] shrink-0" />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search shortcuts…"
          className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-[13px] outline-none"
        />
      </div>

      {groups.length === 0 ? (
        <div className="text-muted-foreground py-8 text-center text-[13px]">
          No matching shortcuts.
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
