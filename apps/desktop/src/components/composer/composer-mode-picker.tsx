import { memo } from "react";
import { Check, ChevronDown } from "lucide-react";
import {
  ModeIcon,
  PERMISSION_MODE_KEYS,
  modeLabel,
  normalizePermissionMode,
} from "@/components/composer/composer-constants";
import { useI18n } from "@/lib/i18n";
import type { ProviderKind, SessionMode } from "@/lib/orbit-ipc";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { cn } from "@/lib/utils";

/**
 * Isolated Access Permissions Dropdown (`[🛡️ Accept Edits ▾]`).
 * Never subscribes to `composerDrafts`, so typing triggers zero re-renders here.
 */
export const ComposerModePicker = memo(function ComposerModePicker({
  mode,
  onSelectMode,
  provider,
  iconOnly = false,
  disabled = false,
  disabledTitle,
}: {
  mode: SessionMode;
  onSelectMode?: (m: SessionMode) => void;
  provider?: ProviderKind | null;
  iconOnly?: boolean;
  disabled?: boolean;
  disabledTitle?: string;
}) {
  const { t } = useI18n();
  const activeMode = normalizePermissionMode(mode);
  const activeModeLabel = modeLabel(activeMode, t);
  const customModeName = typeof activeMode === "object" ? activeMode.custom : null;

  if (disabled) {
    return (
      <SimpleTooltip label={disabledTitle ?? activeModeLabel} side="top">
        <span className="inline-flex shrink-0">
          <button
            type="button"
            disabled
            aria-label={t("composer.accessPermissionsAria", { mode: activeModeLabel })}
            className={cn(
              "text-ink-secondary inline-flex h-8.5 shrink-0 cursor-not-allowed items-center rounded-full text-[13px] font-normal opacity-60",
              iconOnly ? "justify-center gap-0.5 px-2" : "gap-1.5 px-2.5",
            )}
          >
            <ModeIcon mode={activeMode} className="opacity-75" />
            {!iconOnly && <span className="whitespace-nowrap">{activeModeLabel}</span>}
            <ChevronDown className="text-ink-faint size-3.5 shrink-0 stroke-[1.75]" />
          </button>
        </span>
      </SimpleTooltip>
    );
  }

  return (
    <DropdownMenu>
      <SimpleTooltip label={activeModeLabel} shortcut="⇧Tab" side="top">
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t("composer.accessPermissionsAria", { mode: activeModeLabel })}
            className={cn(
              "text-ink-secondary hover:text-ink-primary hover:bg-accent inline-flex h-8.5 shrink-0 cursor-pointer items-center rounded-full text-[13px] font-normal transition-colors",
              iconOnly ? "justify-center gap-0.5 px-2" : "gap-1.5 px-2.5",
            )}
          >
            <ModeIcon mode={activeMode} className="opacity-75" />
            {!iconOnly && <span className="whitespace-nowrap">{activeModeLabel}</span>}
            <ChevronDown className="text-ink-faint size-3.5 shrink-0 stroke-[1.75]" />
          </button>
        </DropdownMenuTrigger>
      </SimpleTooltip>
      <DropdownMenuContent
        align="end"
        side="top"
        className="w-[356px] max-w-[calc(100vw-24px)] p-1.5"
      >
        <DropdownMenuGroup>
          <DropdownMenuLabel>{t("composer.accessPermissions")}</DropdownMenuLabel>

          {customModeName && (
            <DropdownMenuItem
              onSelect={() => onSelectMode?.({ custom: customModeName })}
              className="items-start gap-3 px-2.5 py-2"
            >
              <div className="flex min-w-0 flex-1 items-start gap-2.5">
                <ModeIcon
                  mode={activeMode}
                  className="text-ink-primary mt-0.5 size-4 shrink-0"
                />
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="text-ink-primary text-[13px] leading-tight font-medium">
                    {customModeName}
                  </span>
                  <span className="text-ink-muted text-[11px] leading-snug">
                    {t("common.custom")}
                  </span>
                </div>
              </div>
              <Check className="text-ink-primary mt-0.5 size-3.5 shrink-0" />
            </DropdownMenuItem>
          )}

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
                      selected ? "text-ink-primary" : "text-ink-muted",
                    )}
                  />
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="text-ink-primary text-[13px] leading-tight font-medium">
                      {t(m.labelKey)}
                    </span>
                    <span className="text-ink-muted text-[11px] leading-snug [text-wrap:pretty]">
                      {t(m.descKey)}
                    </span>
                  </div>
                </div>
                {selected && <Check className="text-ink-primary mt-0.5 size-3.5 shrink-0" />}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuGroup>

        {provider === "pi" && (
          <div className="text-ink-faint px-2.5 pt-1 pb-1.5 text-[11px] leading-snug [text-wrap:pretty]">
            {t("composer.piModeLimitedHint")}
          </div>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
});
