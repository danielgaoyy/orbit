import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import { AlertTriangle, ArrowLeftRight, RotateCcw, Search } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isMacDesktop } from "@/lib/orbit-ipc";
import {
  SHORTCUT_ACTIONS,
  type ShortcutActionDefinition,
  type ShortcutActionId,
  eventToChord,
  findShortcutConflicts,
  formatChordForDisplay,
  getEffectiveChord,
  isOsReservedChord,
  swapShortcutBinding,
} from "@/lib/shortcut-registry";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

interface DisplayShortcutRow {
  group: string;
  action: string;
  keys: string;
  actionId?: ShortcutActionId;
  settingId?: string;
  isCustomized?: boolean;
}

interface PendingConflictState {
  actionId: ShortcutActionId;
  candidateChord: string;
  conflict: ShortcutActionDefinition;
}

export function ShortcutsSection({ jumpTo }: { jumpTo?: string }) {
  const { locale, t } = useI18n();
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const customKeybindings = useOrbitSelector((s) => s.customKeybindings);

  const [query, setQuery] = useState("");
  const [recordingId, setRecordingId] = useState<ShortcutActionId | null>(null);
  const [reservedWarning, setReservedWarning] = useState<{
    actionId: ShortcutActionId;
    chord: string;
  } | null>(null);
  const [pendingConflict, setPendingConflict] = useState<PendingConflictState | null>(null);

  const isMac = isMacDesktop();
  const mod = modSymbol();
  const shift = isMac ? "⇧" : "Shift+";

  const hasAnyCustom = Object.keys(customKeybindings).length > 0;

  useLayoutEffect(() => {
    if (jumpTo) setQuery("");
  }, [jumpTo]);

  // Capture live chord while recording
  useEffect(() => {
    if (!recordingId) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        setRecordingId(null);
        setReservedWarning(null);
        return;
      }

      const chord = eventToChord(e, isMac);
      if (!chord) return;

      if (isOsReservedChord(chord)) {
        setReservedWarning({ actionId: recordingId, chord });
        setPendingConflict(null);
        return;
      }

      const conflicts = findShortcutConflicts(recordingId, chord, customKeybindings);
      if (conflicts.length > 0) {
        setReservedWarning(null);
        setPendingConflict({
          actionId: recordingId,
          candidateChord: chord,
          conflict: conflicts[0],
        });
        setRecordingId(null);
        return;
      }

      orbitActions.setCustomKeybinding(recordingId, chord);
      setRecordingId(null);
      setReservedWarning(null);
      setPendingConflict(null);
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recordingId, isMac, customKeybindings]);

  const groups = useMemo(() => {
    const navGroup = t("settings.groupNavigation");
    const composerGroup = t("settings.groupComposer");
    const approvalsGroup = t("settings.groupApprovals");

    const rows: DisplayShortcutRow[] = SHORTCUT_ACTIONS.map((def) => {
      const effectiveChord = getEffectiveChord(def.id, customKeybindings);
      const isCustomized = effectiveChord !== def.defaultChord;
      const displayPrimary = formatChordForDisplay(effectiveChord, isMac);
      const displayKeys =
        !isCustomized && def.altDefaultChord
          ? `${displayPrimary} / ${formatChordForDisplay(def.altDefaultChord, isMac)}`
          : displayPrimary;

      return {
        group: t(def.groupKey),
        action: t(def.labelKey),
        keys: displayKeys,
        actionId: def.id,
        settingId: def.labelKey,
        isCustomized,
      };
    });

    // Read-only contextual shortcuts (fixed behavior / range chords)
    rows.push(
      { group: navGroup, action: t("settings.scSwitchConversation"), keys: `${mod}1 .. ${mod}9` },
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
      { group: composerGroup, action: t("settings.scSteerAgent"), keys: enterToSend ? `${mod}Enter` : `${mod}${shift}Enter` },
      { group: composerGroup, action: t("settings.scStopGeneration"), keys: "Esc Esc" },
      { group: approvalsGroup, action: t("settings.scQuickSelectOption"), keys: "1 .. 9" },
    );

    const q = query.trim().toLowerCase();
    const filtered = q
      ? rows.filter(
          (b) =>
            b.action.toLowerCase().includes(q) ||
            b.group.toLowerCase().includes(q) ||
            b.keys.toLowerCase().includes(q),
        )
      : rows;

    const map = new Map<string, DisplayShortcutRow[]>();
    for (const item of filtered) {
      const list = map.get(item.group) ?? [];
      list.push(item);
      map.set(item.group, list);
    }
    return Array.from(map.entries());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale, isMac, mod, shift, query, enterToSend, customKeybindings]);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-start justify-between gap-3">
        <SettingSectionHeader
          title={t("settings.shortcutsTitle")}
          description={t("settings.shortcutsDesc")}
        />
        {hasAnyCustom && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              orbitActions.resetAllCustomKeybindings();
              setRecordingId(null);
              setReservedWarning(null);
              setPendingConflict(null);
            }}
            className="shrink-0 gap-1.5 text-[13px]"
          >
            <RotateCcw className="size-3" />
            {t("settings.shortcutsResetAll")}
          </Button>
        )}
      </div>

      <div className="relative" data-setting-id="settings.searchShortcutsPlaceholder">
        <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-3 size-[14px] -translate-y-1/2" />
        <Input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label={t("settings.searchShortcutsPlaceholder")}
          placeholder={t("settings.searchShortcutsPlaceholder")}
          className="h-[34px] rounded-[10px] pl-8.5 text-[13px]"
        />
      </div>

      {groups.length === 0 ? (
        <div className="text-ink-muted py-8 text-center text-[13px]">
          {t("settings.noMatchingShortcuts")}
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          {groups.map(([groupName, items]) => (
            <div key={groupName} className="flex flex-col gap-2">
              <SettingGroupLabel>{groupName}</SettingGroupLabel>
              <SettingCard>
                {items.map((item) => {
                  const isRecording = Boolean(item.actionId && recordingId === item.actionId);
                  const showReserved =
                    Boolean(item.actionId && reservedWarning?.actionId === item.actionId);
                  const showConflict =
                    Boolean(item.actionId && pendingConflict?.actionId === item.actionId);

                  return (
                    <div
                      key={item.actionId ?? item.action}
                      data-setting-id={item.settingId}
                      data-modified={item.isCustomized || undefined}
                      className="group/shortcut relative flex flex-col"
                    >
                      <div className="flex items-center justify-between gap-4 px-4 py-2.5">
                        <div className="flex min-w-0 items-center gap-2">
                          <span className="text-ink-primary truncate text-[13px]">
                            {item.action}
                          </span>
                          {item.isCustomized && (
                            <span className="bg-surface-recessed text-ink-muted rounded px-1.5 py-0.5 text-[11px] font-medium">
                              {t("settings.shortcutsCustomBadge")}
                            </span>
                          )}
                        </div>

                        <div className="flex shrink-0 items-center gap-1.5">
                          {item.actionId && item.isCustomized && !isRecording && (
                            <button
                              type="button"
                              title={t("settings.shortcutsResetSingle")}
                              aria-label={t("settings.shortcutsResetSingle")}
                              onClick={() => {
                                orbitActions.resetCustomKeybinding(item.actionId!);
                                setPendingConflict((cur) =>
                                  cur?.actionId === item.actionId ? null : cur,
                                );
                              }}
                              className="text-ink-faint hover:text-ink-primary hover:bg-surface-recessed inline-flex size-[24px] cursor-pointer items-center justify-center rounded-[6px] opacity-0 group-hover/shortcut:opacity-100 group-focus-within/shortcut:opacity-100 focus-visible:opacity-100"
                            >
                              <RotateCcw className="size-[12px]" />
                            </button>
                          )}

                          {item.actionId ? (
                            <button
                              type="button"
                              data-orbit-shortcut-recorder={isRecording ? "true" : undefined}
                              onClick={() => {
                                setReservedWarning(null);
                                setPendingConflict(null);
                                setRecordingId((cur) =>
                                  cur === item.actionId ? null : item.actionId!,
                                );
                              }}
                              onBlur={() => {
                                if (isRecording) setRecordingId(null);
                              }}
                              title={t("settings.shortcutsClickToRecord")}
                              className={cn(
                                "inline-flex h-[26px] min-w-[68px] cursor-pointer items-center justify-center rounded-[7px] border px-2 font-mono text-[11px] transition-colors",
                                isRecording
                                  ? "border-border-strong bg-surface-raised text-ink-primary ring-border-strong ring-1"
                                  : "border-border-subtle bg-surface-recessed text-ink-secondary hover:border-border-default hover:text-ink-primary",
                              )}
                            >
                              {isRecording ? (
                                <span className="font-sans text-[11px] font-medium">
                                  {t("settings.shortcutsPressKeys")}
                                </span>
                              ) : (
                                item.keys
                              )}
                            </button>
                          ) : (
                            <ShortcutKbd keys={item.keys} className="shrink-0" />
                          )}
                        </div>
                      </div>

                      {showReserved && reservedWarning && (
                        <div className="border-border-subtle bg-signal-warning-bg text-signal-warning flex items-center justify-between gap-2 border-t px-4 py-2 text-[13px]">
                          <span className="flex items-center gap-1.5">
                            <AlertTriangle className="size-3.5 shrink-0" />
                            <span>
                              {t("settings.shortcutsReservedError", {
                                keys: formatChordForDisplay(reservedWarning.chord, isMac),
                              })}
                            </span>
                          </span>
                        </div>
                      )}

                      {showConflict && pendingConflict && (
                        <div className="border-border-subtle bg-signal-warning-bg text-signal-warning flex flex-wrap items-center justify-between gap-2 border-t px-4 py-2 text-[13px]">
                          <span className="flex min-w-0 items-center gap-1.5">
                            <AlertTriangle className="size-3.5 shrink-0" />
                            <span className="truncate">
                              {t("settings.shortcutsConflictNotice", {
                                keys: formatChordForDisplay(pendingConflict.candidateChord, isMac),
                                action: t(pendingConflict.conflict.labelKey),
                              })}
                            </span>
                          </span>
                          <div className="flex shrink-0 items-center gap-1.5">
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => setPendingConflict(null)}
                              className="h-[24px] px-2 text-[11px]"
                            >
                              {t("common.cancel")}
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              onClick={() => {
                                const swapped = swapShortcutBinding(
                                  pendingConflict.actionId,
                                  pendingConflict.candidateChord,
                                  pendingConflict.conflict.id,
                                  customKeybindings,
                                );
                                orbitActions.setCustomKeybindings(swapped);
                                setPendingConflict(null);
                              }}
                              className="h-[24px] gap-1 px-2 text-[11px]"
                            >
                              <ArrowLeftRight className="size-3" />
                              {t("settings.shortcutsSwap")}
                            </Button>
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })}
              </SettingCard>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
