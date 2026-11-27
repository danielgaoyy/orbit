import { OrbitSidePanelTab } from '../../model/stores/OrbitSidePanelStore';
import { OrbitSubagentCrewPanel } from './OrbitSubagentCrewPanel';
import {
  OrbitScopedAutomationsTabView,
  OrbitScopedBoardTabView,
  OrbitScopedFileViewerTabView,
  OrbitScopedFilesTabView,
  OrbitScopedGitTabView,
  OrbitScopedSideQuestionTabView,
  OrbitScopedTerminalTabView
} from './OrbitScopedToolTabs';
import {
  OrbitScopedBriefingTabView,
  OrbitScopedPeerTabView,
  OrbitScopedSubagentTabView
} from './OrbitScopedAgentTabs';

/**
 * Controlled content router for all 10 `OrbitSidePanelKind` tab types.
 * Passes both `scopeKey` and explicit `tab` downward so every child isolates transient state
 * cleanly across scope switches and same-kind tab switches, reading ONLY `tab.*`.
 */
@ComponentV2
export struct OrbitSidePanelTabContent {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  build() {
    Column() {
      if (this.tab.kind === 'review') {
        OrbitScopedGitTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'files') {
        OrbitScopedFilesTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'file') {
        OrbitScopedFileViewerTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'terminal') {
        OrbitScopedTerminalTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'board') {
        OrbitScopedBoardTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'side_question') {
        OrbitScopedSideQuestionTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'subagent') {
        if (this.tab.tabId.startsWith('crew:')) {
          OrbitSubagentCrewPanel({ tab: this.tab, scopeKey: this.scopeKey })
        } else {
          OrbitScopedSubagentTabView({ tab: this.tab, scopeKey: this.scopeKey })
        }
      } else if (this.tab.kind === 'peer') {
        OrbitScopedPeerTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else if (this.tab.kind === 'briefing') {
        OrbitScopedBriefingTabView({ tab: this.tab, scopeKey: this.scopeKey })
      } else {
        OrbitScopedAutomationsTabView({ tab: this.tab, scopeKey: this.scopeKey })
      }
    }
    .width('100%')
    .layoutWeight(1)
  }
}
