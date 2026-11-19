import {
  ArrowUpRight,
  Clock,
  FileText,
  GitCompareArrows,
  ListTodo,
  ShieldCheck,
  type LucideIcon,
} from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type SlashDirective, orbitActions, useOrbitSelector } from "@/state/orbit-store";

interface AutomationTemplate {
  id: string;
  title: string;
  description: string;
  cadenceLabel: string;
  icon: LucideIcon;
  slash?: SlashDirective | null;
  prompt: string;
}

const TEMPLATES: AutomationTemplate[] = [
  {
    id: "diff-audit",
    title: "Working Tree Diff Audit",
    description:
      "Inspect uncommitted changes for regressions, edge cases, and architectural boundary violations.",
    cadenceLabel: "On demand · /review",
    icon: GitCompareArrows,
    slash: "review",
    prompt:
      "Audit the current working tree changes for correctness bugs, edge cases, and adherence to project conventions.",
  },
  {
    id: "repo-brief",
    title: "Daily Repository Brief",
    description:
      "Summarize recent commits, active branches, and unfinished work items across the current workspace.",
    cadenceLabel: "Routine digest",
    icon: FileText,
    prompt:
      "Inspect recent git commits, branch status, and uncommitted changes in this repository, then give me a concise engineering status brief and suggested next steps.",
  },
  {
    id: "quality-gate",
    title: "Build, Lint & Test Verification",
    description:
      "Run static checks and unit tests on the active host and diagnose any failing targets.",
    cadenceLabel: "Health check",
    icon: ShieldCheck,
    prompt:
      "Run the project's standard lint, typecheck, and test commands, and report any warnings or failures with concrete fixes.",
  },
  {
    id: "tech-debt-plan",
    title: "Refactor & Cleanup Plan",
    description:
      "Survey hotspots in the current module and propose an incremental, low-risk refactoring plan.",
    cadenceLabel: "Planning · /plan",
    icon: ListTodo,
    slash: "plan",
    prompt:
      "Survey the codebase for high-complexity modules or duplicated logic and draft a concise, incremental refactoring plan.",
  },
];

function shortWorkspaceName(cwd: string): string {
  if (!cwd || cwd === "~") return "~";
  const clean = cwd.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || "/" : clean;
}

export function AutomationsView() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  return (
    <ScrollArea className="orbit-stage-enter min-h-0 flex-1" viewportClassName="px-6 py-10">
      <div className="mx-auto w-full max-w-[768px] space-y-8">
        {/* Restrained Hero Header */}
        <div className="space-y-2">
          <div className="bg-muted text-foreground/75 mb-3 inline-flex h-[36px] w-[36px] items-center justify-center rounded-xl">
            <Clock className="h-[18px] w-[18px] stroke-[1.75]" />
          </div>
          <h1 className="text-foreground text-[18px] font-semibold tracking-[-0.015em]">
            Automations
          </h1>
          <p className="text-muted-foreground max-w-xl text-[13.5px] leading-relaxed">
            Repeatable engineering workflows and background checks scoped to your active project
            {selectedCwd ? ` (${shortWorkspaceName(selectedCwd)})` : ""}. Select a template below to
            open a pre-configured conversation.
          </p>
        </div>

        {/* Starter Workflow Templates */}
        <div className="space-y-2.5">
          <div className="text-muted-foreground px-0.5 text-[12.5px] font-medium">
            Starter Workflows
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {TEMPLATES.map((tpl) => {
              const Icon = tpl.icon;
              return (
                <button
                  key={tpl.id}
                  type="button"
                  onClick={() =>
                    orbitActions.startNewThreadDraft(
                      selectedHostId ?? undefined,
                      selectedCwd || undefined,
                      tpl.prompt,
                      tpl.slash ?? null,
                    )
                  }
                  className="border-border bg-elevated/55 hover:bg-elevated orbit-press group flex cursor-pointer flex-col justify-between gap-4 rounded-2xl border p-4.5 text-left hover:border-black/[0.12] dark:hover:border-white/[0.14]"
                >
                  <div className="space-y-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="bg-muted text-foreground/75 inline-flex h-[30px] w-[30px] items-center justify-center rounded-lg">
                        <Icon className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
                      </span>
                      <ArrowUpRight className="text-muted-foreground group-hover:text-foreground h-[15px] w-[15px] opacity-0 transition-opacity group-hover:opacity-100" />
                    </div>
                    <div className="text-foreground text-[14px] font-medium">{tpl.title}</div>
                    <div className="text-muted-foreground text-[13px] leading-relaxed">
                      {tpl.description}
                    </div>
                  </div>

                  <div className="text-muted-foreground text-[12px] font-medium">
                    {tpl.cadenceLabel}
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </ScrollArea>
  );
}
