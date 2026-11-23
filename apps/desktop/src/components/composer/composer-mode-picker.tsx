import { memo } from "react";
import { Check, ChevronDown } from "lucide-react";
import {
  ModeIcon,
  PERMISSION_MODE_KEYS,
  modeLabel,
  normalizePermissionMode,
} from "@/components/composer/composer-constants";
import { useI18n } from "@/lib/i18n";
import type { SessionMode } from "@/lib/orbit-ipc";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  Kbd,
  KbdGroup,
} from "@/components/ui/overlays";
import { cn } from "@/lib/utils";

/**
 * Isolated Access Permissions Dropdown (`[🛡️ Accept Edits ▾]`).
 * Never subscribes to `composerDrafts`, so typing triggers zero re-renders here.
 */
export const ComposerModePicker = memo(function ComposerModePicker({
  mode,
  onSelectMode,
  iconOnly = false,
  disabled = false,
  disabledTitle,
}: {
  mode: SessionMode;
  onSelectMode?: (m: SessionMode) => void;
  iconOnly?: boolean;
  disabled?: boolean;
  disabledTitle?: string;
}) {
  const { t } = useI18n();
  const activeMode = normalizePermissionMode(mode);
  const activeModeLabel = modeLabel(activeMode, t);

  if (disabled) {
    return (
      <button
        type="button"
        disabled
        title={disabledTitle ?? activeModeLabel}
        aria-label={t("composer.accessPermissionsAria", { mode: activeModeLabel })}
        className={cn(
          "text-foreground/78 inline-flex h-8.5 shrink-0 cursor-not-allowed items-center rounded-full text-[13.5px] font-normal opacity-60",
          iconOnly ? "justify-center gap-0.5 px-2" : "gap-1.5 px-2.5",
        )}
      >
        <ModeIcon mode={activeMode} className="opacity-75" />
        {!iconOnly && (
          <span className="hidden whitespace-nowrap sm:inline">{activeModeLabel}</span>
        )}
        <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
      </button>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={`${activeModeLabel} (⇧Tab)`}
          aria-label={t("composer.accessPermissionsAria", { mode: activeModeLabel })}
          className={cn(
            "text-foreground/78 hover:text-foreground inline-flex h-8.5 shrink-0 cursor-pointer items-center rounded-full text-[13.5px] font-normal transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            iconOnly ? "justify-center gap-0.5 px-2" : "gap-1.5 px-2.5",
          )}
        >
          <ModeIcon mode={activeMode} className="opacity-75" />
          {!iconOnly && <span className="hidden whitespace-nowrap sm:inline">{activeModeLabel}</span>}
          <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="top" className="w-[368px] p-1.5">
        <DropdownMenuLabel className="flex items-center justify-between">
          <span>{t("composer.accessPermissions")}</span>
          <KbdGroup>
            <Kbd>⇧</Kbd>
            <Kbd>Tab</Kbd>
          </KbdGroup>
        </DropdownMenuLabel>
        {PERMISSION_MODE_KEYS.map((m) => {
          const selected = m.id === activeMode;
          return (
            <DropdownMenuItem
              key={m.id}
              onSelect={() => onSelectMode?.(m.id)}
              className="items-start gap-3 px-2.5 py-2"
            >
              <div className="flex min-w-0 flex-1 items-start gap-2.5">
                <ModeIcon
                  mode={m.id}
                  className={cn(
                    "mt-0.5 size-4 shrink-0",
                    selected ? "text-foreground" : "text-muted-foreground",
                  )}
                />
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="text-foreground text-[13px] leading-tight font-medium whitespace-nowrap">
                    {t(m.labelKey)}
                  </span>
                  <span className="text-muted-foreground text-[11.5px] leading-snug whitespace-nowrap">
                    {t(m.descKey)}
                  </span>
                </div>
              </div>
              {selected && <Check className="text-foreground mt-0.5 size-3.5 shrink-0" />}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
});
