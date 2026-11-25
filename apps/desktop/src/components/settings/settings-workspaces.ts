import { useMemo } from "react";
import { basenameOfProjectPath } from "@/lib/projects";
import { useOrbitSelector } from "@/state/orbit-store";

// Preferences can target a host other than the one presenting the conversation.
// An explicit file link takes priority over that host's current workspace.
export function useSettingsWorkspaces(hostId: string | null, preferredCwd?: string) {
  const fleetProjects = useOrbitSelector((s) => s.fleet.projects);
  const detail = useOrbitSelector((s) => (hostId ? s.hostDetails[hostId] : undefined));
  const selectedHost = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgent = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  return useMemo(() => {
    const paths = new Map<string, string>();
    const add = (path?: string | null, label?: string | null) => {
      const cwd = path?.trim();
      if (cwd && cwd !== "~" && !paths.has(cwd)) {
        paths.set(cwd, label || basenameOfProjectPath(cwd));
      }
    };
    add(preferredCwd);
    if (selectedHost === hostId) {
      add(detail?.agents.find((agent) => agent.agent_id === selectedAgent)?.cwd);
      add(selectedCwd);
    }
    for (const project of fleetProjects ?? []) {
      if (project.host_id === hostId) {
        for (const cwd of project.directories) add(cwd, project.name);
      }
    }
    for (const workspace of detail?.workspaces ?? []) {
      if (!workspace.archived_at) add(workspace.cwd, workspace.title);
    }
    for (const project of detail?.discovered_projects ?? []) add(project.path, project.name);
    for (const agent of detail?.agents ?? []) add(agent.cwd);
    return Array.from(paths, ([value, label]) => ({ value, label, description: value }));
  }, [hostId, detail, fleetProjects, selectedAgent, selectedCwd, selectedHost, preferredCwd]);
}
