import { memo } from "react";
import {
  AgentStagePane,
  recoverReplyDraftAfterSendError,
} from "@/components/stage/agent-stage-pane";
import type { SidePanelTab } from "@/state/orbit-store";

export { recoverReplyDraftAfterSendError };

export const PeerSplitTab = memo(function PeerSplitTab({ tab }: { tab: SidePanelTab }) {
  return (
    <AgentStagePane
      hostId={tab.hostId ?? ""}
      agentId={tab.peerAgentId ?? ""}
      fallbackTitle={tab.title}
      fallbackCwd={tab.cwd}
      showBoardButton
    />
  );
});
