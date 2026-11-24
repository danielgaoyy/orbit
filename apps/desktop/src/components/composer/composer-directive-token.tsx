import { memo } from "react";
import { resolveSlashCommandIcon } from "@/components/composer/slash-menu";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { type ComposerInlineDirective, SLASH_DESC_KEY_BY_ID } from "@/lib/slash-commands";

export const ComposerDirectiveToken = memo(function ComposerDirectiveToken({ directive, onClick }: {
  directive: ComposerInlineDirective;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const Icon = resolveSlashCommandIcon(directive);
  return (
    <SimpleTooltip side="top" label={t(SLASH_DESC_KEY_BY_ID[directive])}>
      <button
        type="button"
        onClick={onClick}
        className="text-ink-primary hover:bg-accent/80 pointer-events-auto inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] px-1.5 py-[1px] text-[15px] leading-[22px] font-medium transition-colors duration-100"
      >
        <Icon aria-hidden="true" className="text-ink-secondary size-3.5 shrink-0" />
        <span>{directive}</span>
      </button>
    </SimpleTooltip>
  );
});
