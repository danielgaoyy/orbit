import { memo } from "react";
import { resolveSlashCommandIcon } from "@/components/composer/slash-menu";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import {
  type ComposerInlineDirective,
  SLASH_DESC_KEY_BY_ID,
  normalizeDirectiveLabel,
} from "@/lib/slash-commands";
import { cn } from "@/lib/utils";

/**
 * Frameless, colorless inline slash-directive token shared across `OmniComposer`,
 * `UserPromptBubble`, and `ComposerQueueStrip`.
 *
 * Uses `items-baseline` on the flex container with `self-center` on the icon so the
 * flex container's baseline anchors to the directive text (CSS Flexbox §4.1) rather
 * than the icon's bottom edge — preventing vertical float against CJK/Latin prose.
 */
export const InlineDirectiveToken = memo(function InlineDirectiveToken({
  directive,
  size = "bubble",
  showTitle = false,
  className,
}: {
  directive: string;
  size?: "composer" | "bubble" | "compact";
  showTitle?: boolean;
  className?: string;
}) {
  const { t } = useI18n();
  const label = normalizeDirectiveLabel(directive);
  const Icon = resolveSlashCommandIcon(label);
  const descKey = showTitle ? SLASH_DESC_KEY_BY_ID[label] : undefined;
  const iconSizeClass = size === "compact" ? "size-[12.5px]" : "size-[14px]";

  return (
    <span
      data-directive-token={label}
      title={descKey ? t(descKey) : undefined}
      className={cn(
        "text-ink-primary inline-flex items-baseline align-baseline font-medium select-none",
        size === "compact" ? "gap-[4px] text-[13px] leading-[18px]" : "gap-[5px]",
        className,
      )}
    >
      <Icon
        aria-hidden="true"
        className={cn("text-ink-secondary shrink-0 self-center stroke-[1.8]", iconSizeClass)}
      />
      <span>{label}</span>
    </span>
  );
});

export const ComposerDirectiveToken = memo(function ComposerDirectiveToken({ directive, onClick }: {
  directive: ComposerInlineDirective;
  onClick: () => void;
}) {
  const { t } = useI18n();
  return (
    <SimpleTooltip side="top" label={t(SLASH_DESC_KEY_BY_ID[directive])}>
      <button
        type="button"
        onClick={onClick}
        className="hover:bg-accent/80 pointer-events-auto inline-flex cursor-pointer items-center rounded-[6px] px-[6px] py-[1px] text-[15px] leading-[22px] transition-colors duration-100"
      >
        <InlineDirectiveToken directive={directive} size="composer" />
      </button>
    </SimpleTooltip>
  );
});
