import {
  ConditionalTimerRecordDto,
  GoalCriterionDto,
  GoalRunRecordDto,
  MonitorWatchRecordDto,
  ScheduleRunRecordDto,
  ScheduleSpecDto,
  WorkflowRunRecordDto,
  WorkflowStepRecordDto,
  formatProviderLabel,
  formatTimerConditionLabel,
  formatWorkflowKindLabel
} from '../../model/OrbitTypes';
import {
  OrbitHarmonyStore,
  OrbitScopedAutomationsData
} from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import {
  OrbitSidePanelTab,
  encodeSidePanelKeyTuple
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

interface ScopedAutomationTabSpec {
  idx: number;
  label: Resource;
}

const SCOPED_CRON_PRESETS: string[] = ['*/15 * * * *', '0 * * * *', '0 9 * * *'];
const SCOPED_AUTOMATION_TABS: ScopedAutomationTabSpec[] = [
  { idx: 0, label: $r('app.string.automations_schedules') },
  { idx: 1, label: $r('app.string.automations_timers') },
  { idx: 2, label: $r('app.string.automations_goals') },
  { idx: 3, label: $r('app.string.automations_monitors') },
  { idx: 4, label: $r('app.string.automations_workflows') }
];

type OrbitAutomationFormKind = 'schedule' | 'timer' | 'goal' | 'monitor' | 'workflow';

interface OrbitAutomationDraftSnapshot {
  scheduleEditVersion: number;
  timerEditVersion: number;
  goalEditVersion: number;
  monitorEditVersion: number;
  workflowEditVersion: number;
  showScheduleForm: boolean;
  draftSchedName: string;
  draftSchedCron: string;
  draftSchedPrompt: string;
  draftSchedWorktree: boolean;
  showTimerForm: boolean;
  draftTimerSecs: string;
  draftTimerCond: string;
  draftTimerPrompt: string;
  showGoalForm: boolean;
  draftGoalObjective: string;
  draftGoalVerifyCmd: string;
  draftGoalMaxIter: string;
  showMonitorForm: boolean;
  draftMonitorName: string;
  draftMonitorKind: string;
  draftMonitorTarget: string;
  draftMonitorPattern: string;
  showWorkflowForm: boolean;
  draftWorkflowName: string;
  draftWorkflowPrompt: string;
}

function defaultAutomationDraftSnapshot(): OrbitAutomationDraftSnapshot {
  return {
    scheduleEditVersion: 0,
    timerEditVersion: 0,
    goalEditVersion: 0,
    monitorEditVersion: 0,
    workflowEditVersion: 0,
    showScheduleForm: false,
    draftSchedName: '',
    draftSchedCron: '0 9 * * *',
    draftSchedPrompt: '',
    draftSchedWorktree: false,
    showTimerForm: false,
    draftTimerSecs: '300',
    draftTimerCond: 'never',
    draftTimerPrompt: '',
    showGoalForm: false,
    draftGoalObjective: '',
    draftGoalVerifyCmd: '',
    draftGoalMaxIter: '6',
    showMonitorForm: false,
    draftMonitorName: '',
    draftMonitorKind: 'command_stream',
    draftMonitorTarget: '',
    draftMonitorPattern: '',
    showWorkflowForm: false,
    draftWorkflowName: '',
    draftWorkflowPrompt: ''
  };
}

function emptyAutomationsData(): OrbitScopedAutomationsData {
  return {
    schedules: [],
    timers: [],
    goals: [],
    monitors: [],
    workflows: []
  };
}

/**
 * Scoped Automations tab (`kind === 'automations'`).
 * - Strictly bound to `(tab.hostId, tab.agentId, tab.cwd)` — never reads global host/agent/cwd.
 * - P1-1: Increments `loadSeq` and clears stale data/forms BEFORE returning on empty host or tab switch.
 * - P1-8: Restores full 5-category management (Schedules, Timers, Goals, Monitors, Workflows)
 *   including creation forms, control actions, schedule run history, and per-tab empty states.
 */
@ComponentV2
export struct OrbitScopedAutomationsTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private activeTabIndex: number = 0;
  @Local private data: OrbitScopedAutomationsData = emptyAutomationsData();
  @Local private loading: boolean = false;
  @Local private expandedScheduleRunsId: string = '';
  @Local private expandedScheduleRuns: ScheduleRunRecordDto[] = [];

  // Schedule create form
  @Local private showScheduleForm: boolean = false;
  @Local private draftSchedName: string = '';
  @Local private draftSchedCron: string = '0 9 * * *';
  @Local private draftSchedPrompt: string = '';
  @Local private draftSchedWorktree: boolean = false;

  // Timer create form
  @Local private showTimerForm: boolean = false;
  @Local private draftTimerSecs: string = '300';
  @Local private draftTimerCond: string = 'never';
  @Local private draftTimerPrompt: string = '';

  // Goal create form
  @Local private showGoalForm: boolean = false;
  @Local private draftGoalObjective: string = '';
  @Local private draftGoalVerifyCmd: string = '';
  @Local private draftGoalMaxIter: string = '6';

  // Monitor create form
  @Local private showMonitorForm: boolean = false;
  @Local private draftMonitorName: string = '';
  @Local private draftMonitorKind: string = 'command_stream';
  @Local private draftMonitorTarget: string = '';
  @Local private draftMonitorPattern: string = '';

  // Workflow create form
  @Local private showWorkflowForm: boolean = false;
  @Local private draftWorkflowName: string = '';
  @Local private draftWorkflowPrompt: string = '';

  // Per-form submitting flags bound to the currently active target (R3-P1-10)
  @Local private submittingSchedule: boolean = false;
  @Local private submittingTimer: boolean = false;
  @Local private submittingGoal: boolean = false;
  @Local private submittingMonitor: boolean = false;
  @Local private submittingWorkflow: boolean = false;

  private draftsByTargetKey: Map<string, OrbitAutomationDraftSnapshot> =
    new Map<string, OrbitAutomationDraftSnapshot>();
  private lastTargetKey: string = '';
  private loadSeq: number = 0;
  private scheduleEditVersion: number = 0;
  private timerEditVersion: number = 0;
  private goalEditVersion: number = 0;
  private monitorEditVersion: number = 0;
  private workflowEditVersion: number = 0;
  private disposed: boolean = false;
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;

  private effectiveScopeKey(): string {
    const clean = this.scopeKey.trim();
    return clean.length > 0 ? clean : this.store.sidePanelStore.activeScopeKey;
  }

  private currentTargetKey(): string {
    return encodeSidePanelKeyTuple([
      'automations',
      this.effectiveScopeKey(),
      this.tab.tabId,
      this.tab.hostId,
      this.tab.agentId ?? '',
      this.tab.cwd ?? ''
    ]);
  }

  private submitLockKey(kind: OrbitAutomationFormKind): string {
    return this.store.automationSubmitLockKey(
      this.effectiveScopeKey(),
      this.tab.tabId,
      this.tab.hostId,
      this.tab.agentId,
      this.tab.cwd,
      kind
    );
  }

  private syncSubmittingFlagsForCurrentTarget(): void {
    this.submittingSchedule = this.store.isAutomationFormSubmitting(this.submitLockKey('schedule'));
    this.submittingTimer = this.store.isAutomationFormSubmitting(this.submitLockKey('timer'));
    this.submittingGoal = this.store.isAutomationFormSubmitting(this.submitLockKey('goal'));
    this.submittingMonitor = this.store.isAutomationFormSubmitting(this.submitLockKey('monitor'));
    this.submittingWorkflow = this.store.isAutomationFormSubmitting(this.submitLockKey('workflow'));
  }

  private captureDraftSnapshot(): OrbitAutomationDraftSnapshot {
    return {
      scheduleEditVersion: this.scheduleEditVersion,
      timerEditVersion: this.timerEditVersion,
      goalEditVersion: this.goalEditVersion,
      monitorEditVersion: this.monitorEditVersion,
      workflowEditVersion: this.workflowEditVersion,
      showScheduleForm: this.showScheduleForm,
      draftSchedName: this.draftSchedName,
      draftSchedCron: this.draftSchedCron,
      draftSchedPrompt: this.draftSchedPrompt,
      draftSchedWorktree: this.draftSchedWorktree,
      showTimerForm: this.showTimerForm,
      draftTimerSecs: this.draftTimerSecs,
      draftTimerCond: this.draftTimerCond,
      draftTimerPrompt: this.draftTimerPrompt,
      showGoalForm: this.showGoalForm,
      draftGoalObjective: this.draftGoalObjective,
      draftGoalVerifyCmd: this.draftGoalVerifyCmd,
      draftGoalMaxIter: this.draftGoalMaxIter,
      showMonitorForm: this.showMonitorForm,
      draftMonitorName: this.draftMonitorName,
      draftMonitorKind: this.draftMonitorKind,
      draftMonitorTarget: this.draftMonitorTarget,
      draftMonitorPattern: this.draftMonitorPattern,
      showWorkflowForm: this.showWorkflowForm,
      draftWorkflowName: this.draftWorkflowName,
      draftWorkflowPrompt: this.draftWorkflowPrompt
    };
  }

  private applyDraftSnapshot(snap: OrbitAutomationDraftSnapshot): void {
    this.scheduleEditVersion = snap.scheduleEditVersion;
    this.timerEditVersion = snap.timerEditVersion;
    this.goalEditVersion = snap.goalEditVersion;
    this.monitorEditVersion = snap.monitorEditVersion;
    this.workflowEditVersion = snap.workflowEditVersion;
    this.showScheduleForm = snap.showScheduleForm;
    this.draftSchedName = snap.draftSchedName;
    this.draftSchedCron = snap.draftSchedCron;
    this.draftSchedPrompt = snap.draftSchedPrompt;
    this.draftSchedWorktree = snap.draftSchedWorktree;
    this.showTimerForm = snap.showTimerForm;
    this.draftTimerSecs = snap.draftTimerSecs;
    this.draftTimerCond = snap.draftTimerCond;
    this.draftTimerPrompt = snap.draftTimerPrompt;
    this.showGoalForm = snap.showGoalForm;
    this.draftGoalObjective = snap.draftGoalObjective;
    this.draftGoalVerifyCmd = snap.draftGoalVerifyCmd;
    this.draftGoalMaxIter = snap.draftGoalMaxIter;
    this.showMonitorForm = snap.showMonitorForm;
    this.draftMonitorName = snap.draftMonitorName;
    this.draftMonitorKind = snap.draftMonitorKind;
    this.draftMonitorTarget = snap.draftMonitorTarget;
    this.draftMonitorPattern = snap.draftMonitorPattern;
    this.showWorkflowForm = snap.showWorkflowForm;
    this.draftWorkflowName = snap.draftWorkflowName;
    this.draftWorkflowPrompt = snap.draftWorkflowPrompt;
  }

  private markFormEdited(kind: OrbitAutomationFormKind): void {
    if (kind === 'schedule') {
      this.scheduleEditVersion += 1;
    } else if (kind === 'timer') {
      this.timerEditVersion += 1;
    } else if (kind === 'goal') {
      this.goalEditVersion += 1;
    } else if (kind === 'monitor') {
      this.monitorEditVersion += 1;
    } else {
      this.workflowEditVersion += 1;
    }
    const key = this.currentTargetKey();
    this.draftsByTargetKey.set(key, this.captureDraftSnapshot());
  }

  private switchTargetState(prevKey: string, nextKey: string): void {
    // Save outgoing target's drafts & independent per-kind edit versions under its own key so
    // drafts and parallel submissions NEVER cross hosts/agents or interfere across forms (R2-P0-3 + R3-P1-10).
    if (prevKey.length > 0) {
      this.draftsByTargetKey.set(prevKey, this.captureDraftSnapshot());
    }
    this.lastTargetKey = nextKey;
    this.loadSeq += 1;
    this.data = emptyAutomationsData();
    this.loading = false;
    this.expandedScheduleRunsId = '';
    this.expandedScheduleRuns = [];
    const saved = this.draftsByTargetKey.get(nextKey);
    if (saved !== undefined) {
      this.applyDraftSnapshot(saved);
    } else {
      this.applyDraftSnapshot(defaultAutomationDraftSnapshot());
    }
    this.syncSubmittingFlagsForCurrentTarget();
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.lastTargetKey = this.currentTargetKey();
    const saved = this.draftsByTargetKey.get(this.lastTargetKey);
    if (saved !== undefined) {
      this.applyDraftSnapshot(saved);
    } else {
      this.applyDraftSnapshot(defaultAutomationDraftSnapshot());
    }
    this.syncSubmittingFlagsForCurrentTarget();
    void this.refresh();
  }

  aboutToDisappear(): void {
    this.disposed = true;
    this.loadSeq += 1;
    this.scheduleEditVersion += 1;
    this.timerEditVersion += 1;
    this.goalEditVersion += 1;
    this.monitorEditVersion += 1;
    this.workflowEditVersion += 1;
  }

  @Monitor('tab', 'scopeKey', 'store.panelDataRevision', 'store.automationSubmitRevision', 'fleetStore.hostDetailRevision')
  onTabOrDataChanged(_mon: IMonitor): void {
    const nextKey = this.currentTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.switchTargetState(this.lastTargetKey, nextKey);
      void this.refresh();
      return;
    }
    this.syncSubmittingFlagsForCurrentTarget();
    void this.refresh();
  }

  private async refresh(): Promise<void> {
    const host = this.tab.hostId.trim();
    const targetKey = this.currentTargetKey();
    this.loadSeq += 1;
    const seq = this.loadSeq;
    if (host.length === 0) {
      this.data = emptyAutomationsData();
      this.loading = false;
      return;
    }
    this.loading = true;
    const loaded = await this.store.loadScopedAutomations(host);
    if (!this.disposed && seq === this.loadSeq && targetKey === this.currentTargetKey()) {
      this.data = loaded;
      this.loading = false;
    }
  }

  private submitScheduleCreate(): void {
    const targetKey = this.currentTargetKey();
    const lockKey = this.submitLockKey('schedule');
    const token = this.store.acquireAutomationSubmitToken(lockKey);
    if (token === 0) {
      return;
    }
    const hostId = this.tab.hostId;
    const cwd = this.tab.cwd ?? '~';
    const submitVersion = this.scheduleEditVersion;
    const name = this.draftSchedName;
    const cron = this.draftSchedCron;
    const prompt = this.draftSchedPrompt;
    const worktree = this.draftSchedWorktree;
    this.syncSubmittingFlagsForCurrentTarget();
    try {
      dismissHarmonyKeyboard(this.getUIContext());
      const ok = this.store.createScopedSchedule(hostId, cwd, name, cron, prompt, worktree);
      if (!ok) {
        return;
      }
      if (!this.disposed && targetKey === this.currentTargetKey() && submitVersion === this.scheduleEditVersion) {
        this.draftSchedName = '';
        this.draftSchedPrompt = '';
        this.showScheduleForm = false;
        this.markFormEdited('schedule');
        void this.refresh();
      } else {
        const saved = this.draftsByTargetKey.get(targetKey);
        if (saved !== undefined && saved.scheduleEditVersion === submitVersion) {
          saved.draftSchedName = '';
          saved.draftSchedPrompt = '';
          saved.showScheduleForm = false;
          saved.scheduleEditVersion += 1;
        }
      }
    } finally {
      this.store.releaseAutomationSubmitToken(lockKey, token);
      if (!this.disposed) {
        this.syncSubmittingFlagsForCurrentTarget();
      }
    }
  }

  private async submitTimerCreate(): Promise<void> {
    const targetKey = this.currentTargetKey();
    const lockKey = this.submitLockKey('timer');
    const token = this.store.acquireAutomationSubmitToken(lockKey);
    if (token === 0) {
      return;
    }
    const hostId = this.tab.hostId;
    const agentId = this.tab.agentId;
    const submitVersion = this.timerEditVersion;
    const secs = Number(this.draftTimerSecs);
    const prompt = this.draftTimerPrompt;
    const cond = this.draftTimerCond;
    this.syncSubmittingFlagsForCurrentTarget();
    try {
      dismissHarmonyKeyboard(this.getUIContext());
      const ok = await this.store.createScopedConditionalTimer(
        hostId,
        agentId,
        Number.isFinite(secs) && secs >= 10 ? secs : 300,
        prompt,
        cond
      );
      if (!ok) {
        return;
      }
      if (!this.disposed && targetKey === this.currentTargetKey() && submitVersion === this.timerEditVersion) {
        this.draftTimerPrompt = '';
        this.showTimerForm = false;
        this.markFormEdited('timer');
        void this.refresh();
      } else {
        const saved = this.draftsByTargetKey.get(targetKey);
        if (saved !== undefined && saved.timerEditVersion === submitVersion) {
          saved.draftTimerPrompt = '';
          saved.showTimerForm = false;
          saved.timerEditVersion += 1;
        }
      }
    } finally {
      this.store.releaseAutomationSubmitToken(lockKey, token);
      if (!this.disposed) {
        this.syncSubmittingFlagsForCurrentTarget();
      }
    }
  }

  private async submitGoalCreate(): Promise<void> {
    const targetKey = this.currentTargetKey();
    const lockKey = this.submitLockKey('goal');
    const token = this.store.acquireAutomationSubmitToken(lockKey);
    if (token === 0) {
      return;
    }
    const hostId = this.tab.hostId;
    const agentId = this.tab.agentId;
    const submitVersion = this.goalEditVersion;
    const objective = this.draftGoalObjective;
    const verifyCmd = this.draftGoalVerifyCmd.trim().length > 0 ? this.draftGoalVerifyCmd.trim() : null;
    const maxIt = Number(this.draftGoalMaxIter);
    this.syncSubmittingFlagsForCurrentTarget();
    try {
      dismissHarmonyKeyboard(this.getUIContext());
      const ok = await this.store.startScopedGoalRun(
        hostId,
        agentId,
        objective,
        verifyCmd,
        Number.isFinite(maxIt) && maxIt >= 1 ? maxIt : 6
      );
      if (!ok) {
        return;
      }
      if (!this.disposed && targetKey === this.currentTargetKey() && submitVersion === this.goalEditVersion) {
        this.draftGoalObjective = '';
        this.draftGoalVerifyCmd = '';
        this.showGoalForm = false;
        this.markFormEdited('goal');
        void this.refresh();
      } else {
        const saved = this.draftsByTargetKey.get(targetKey);
        if (saved !== undefined && saved.goalEditVersion === submitVersion) {
          saved.draftGoalObjective = '';
          saved.draftGoalVerifyCmd = '';
          saved.showGoalForm = false;
          saved.goalEditVersion += 1;
        }
      }
    } finally {
      this.store.releaseAutomationSubmitToken(lockKey, token);
      if (!this.disposed) {
        this.syncSubmittingFlagsForCurrentTarget();
      }
    }
  }

  private async submitMonitorCreate(): Promise<void> {
    const targetKey = this.currentTargetKey();
    const lockKey = this.submitLockKey('monitor');
    const token = this.store.acquireAutomationSubmitToken(lockKey);
    if (token === 0) {
      return;
    }
    const hostId = this.tab.hostId;
    const agentId = this.tab.agentId;
    const submitVersion = this.monitorEditVersion;
    const name = this.draftMonitorName;
    const kind = this.draftMonitorKind;
    const target = this.draftMonitorTarget;
    const pattern = this.draftMonitorPattern.trim().length > 0 ? this.draftMonitorPattern.trim() : null;
    this.syncSubmittingFlagsForCurrentTarget();
    try {
      dismissHarmonyKeyboard(this.getUIContext());
      const ok = await this.store.createScopedMonitorWatch(hostId, agentId, name, kind, target, pattern);
      if (!ok) {
        return;
      }
      if (!this.disposed && targetKey === this.currentTargetKey() && submitVersion === this.monitorEditVersion) {
        this.draftMonitorName = '';
        this.draftMonitorTarget = '';
        this.draftMonitorPattern = '';
        this.showMonitorForm = false;
        this.markFormEdited('monitor');
        void this.refresh();
      } else {
        const saved = this.draftsByTargetKey.get(targetKey);
        if (saved !== undefined && saved.monitorEditVersion === submitVersion) {
          saved.draftMonitorName = '';
          saved.draftMonitorTarget = '';
          saved.draftMonitorPattern = '';
          saved.showMonitorForm = false;
          saved.monitorEditVersion += 1;
        }
      }
    } finally {
      this.store.releaseAutomationSubmitToken(lockKey, token);
      if (!this.disposed) {
        this.syncSubmittingFlagsForCurrentTarget();
      }
    }
  }

  private async submitWorkflowCreate(): Promise<void> {
    const targetKey = this.currentTargetKey();
    const lockKey = this.submitLockKey('workflow');
    const token = this.store.acquireAutomationSubmitToken(lockKey);
    if (token === 0) {
      return;
    }
    const hostId = this.tab.hostId;
    const cwd = this.tab.cwd ?? '~';
    const submitVersion = this.workflowEditVersion;
    const name = this.draftWorkflowName;
    const prompt = this.draftWorkflowPrompt;
    this.syncSubmittingFlagsForCurrentTarget();
    try {
      dismissHarmonyKeyboard(this.getUIContext());
      const ok = await this.store.startScopedDagWorkflow(hostId, cwd, name, prompt);
      if (!ok) {
        return;
      }
      if (!this.disposed && targetKey === this.currentTargetKey() && submitVersion === this.workflowEditVersion) {
        this.draftWorkflowName = '';
        this.draftWorkflowPrompt = '';
        this.showWorkflowForm = false;
        this.markFormEdited('workflow');
        void this.refresh();
      } else {
        const saved = this.draftsByTargetKey.get(targetKey);
        if (saved !== undefined && saved.workflowEditVersion === submitVersion) {
          saved.draftWorkflowName = '';
          saved.draftWorkflowPrompt = '';
          saved.showWorkflowForm = false;
          saved.workflowEditVersion += 1;
        }
      }
    } finally {
      this.store.releaseAutomationSubmitToken(lockKey, token);
      if (!this.disposed) {
        this.syncSubmittingFlagsForCurrentTarget();
      }
    }
  }

  private statusColor(status: string): ResourceColor {
    if (status === 'running' || status === 'verifying' || status === 'armed' || status === 'active') {
      return this.store.palette.accentPrimary;
    }
    if (status === 'succeeded' || status === 'fired' || status === 'triggered') {
      return this.store.palette.statusDirect;
    }
    if (status === 'paused' || status === 'partial_failure') {
      return this.store.palette.statusWarning;
    }
    if (status === 'failed' || status === 'error') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private formatStatusLabel(raw: string): string {
    if (raw.length === 0) {
      return 'Unknown';
    }
    return raw.replace(/_/g, ' ').replace(/\b\w/g, (c: string): string => c.toUpperCase());
  }

  private formatRemainingTimer(timer: ConditionalTimerRecordDto): string {
    if (timer.state !== 'armed') {
      return this.formatStatusLabel(timer.state);
    }
    const remSec = Math.max(0, Math.round((timer.fire_at_ms - Date.now()) / 1000));
    if (remSec < 60) {
      return `${remSec}s left`;
    }
    const mins = Math.floor(remSec / 60);
    const secs = remSec % 60;
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
  }

  @Computed
  get armedTimers(): ConditionalTimerRecordDto[] {
    return this.data.timers.filter((t: ConditionalTimerRecordDto): boolean => t.state === 'armed');
  }

  @Builder
  private renderEmpty(title: string, subtitle: string) {
    Column({ space: OrbitTokens.SPACE_XS }) {
      Text(title)
        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
      Text(subtitle)
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        .textAlign(TextAlign.Center)
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_LG)
    .backgroundColor(this.store.palette.listCard)
    .borderRadius(OrbitTokens.RADIUS_MD)
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  private renderSchedulesSection() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`${this.data.schedules.length} configured`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Blank()
          Button(this.showScheduleForm ? $r('app.string.action_close') : 'New Schedule', {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showScheduleForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showScheduleForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.showScheduleForm = !this.showScheduleForm;
            })
        }
        .width('100%')
      }

      if (this.showScheduleForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            TextInput({ placeholder: $r('app.string.automations_ph_schedule_name'), text: this.draftSchedName })
              .attributeModifier(
                orbitInputSubhead(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftSchedName = v;
                this.markFormEdited('schedule');
              })

            TextInput({ placeholder: $r('app.string.automations_ph_cron'), text: this.draftSchedCron })
              .attributeModifier(
                orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftSchedCron = v;
                this.markFormEdited('schedule');
              })

            Row({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                SCOPED_CRON_PRESETS,
                (preset: string) => {
                  Button(preset, { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(
                        this.draftSchedCron === preset
                          ? this.store.palette.accentPrimary
                          : this.store.palette.fgSecondary,
                        this.store.palette.subtleSurface
                      )
                    )
                    .height(26)
                    .padding({ left: 8, right: 8 })
                    .onClick(() => {
                      this.draftSchedCron = preset;
                      this.markFormEdited('schedule');
                    })
                },
                (preset: string): string => `sc_pre_${preset}`
              )
            }
            .width('100%')

            TextArea({ placeholder: $r('app.string.automations_ph_trigger_prompt'), text: this.draftSchedPrompt })
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.recessed)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .constraintSize({ minHeight: 56, maxHeight: 110 })
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftSchedPrompt = v;
                this.markFormEdited('schedule');
              })

            Row() {
              Text($r('app.string.automations_run_in_worktree'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .layoutWeight(1)
              Toggle({ type: ToggleType.Switch, isOn: this.draftSchedWorktree })
                .onChange((on: boolean) => {
                  this.draftSchedWorktree = on;
                  this.markFormEdited('schedule');
                })
            }
            .width('100%')

            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .width('100%')
              .height(34)
              .enabled(!this.submittingSchedule)
              .onClick(() => {
                this.submitScheduleCreate();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        }
      }

      if (this.data.schedules.length === 0) {
        ListItem() {
          this.renderEmpty('No schedules yet', 'Create a cron schedule on this host.')
        }
      } else {
        ForEach(
          this.data.schedules,
          (sched: ScheduleSpecDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(sched.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)
                  Text(sched.cron_expr)
                    .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                    .padding({ left: 6, right: 6, top: 2, bottom: 2 })
                    .borderRadius(OrbitTokens.RADIUS_SM)
                    .backgroundColor(this.store.palette.subtleSurface)
                }
                .width('100%')

                Text(sched.prompt)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                  .maxLines(2)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .width('100%')

                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(`${this.formatStatusLabel(sched.status)} · ${sched.run_count} runs`)
                    .attributeModifier(orbitTextMeta(this.statusColor(sched.status)))
                    .layoutWeight(1)

                  if (sched.status === 'paused') {
                    Button('Resume', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface))
                      .height(28)
                      .padding({ left: 9, right: 9 })
                      .onClick(() => {
                        this.store.resumeScopedSchedule(this.tab.hostId, sched.schedule_id);
                        void this.refresh();
                      })
                  } else if (sched.status === 'active') {
                    Button('Pause', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
                      .height(28)
                      .padding({ left: 9, right: 9 })
                      .onClick(() => {
                        this.store.pauseScopedSchedule(this.tab.hostId, sched.schedule_id);
                        void this.refresh();
                      })
                  }

                  Button('Run', { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                    .height(28)
                    .padding({ left: 9, right: 9 })
                    .onClick(() => {
                      this.store.runScopedScheduleOnce(this.tab.hostId, sched.schedule_id);
                    })

                  Button('Runs', { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
                    .height(28)
                    .padding({ left: 9, right: 9 })
                    .onClick(() => {
                      if (this.expandedScheduleRunsId === sched.schedule_id) {
                        this.expandedScheduleRunsId = '';
                        this.expandedScheduleRuns = [];
                      } else {
                        this.expandedScheduleRunsId = sched.schedule_id;
                        this.expandedScheduleRuns = this.store.loadScopedScheduleRuns(this.tab.hostId, sched.schedule_id);
                      }
                    })

                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.trash'))
                      .fontSize(13)
                      .fontColor([this.store.palette.statusDanger])
                  }
                  .width(28)
                  .height(28)
                  .padding(0)
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(`Delete ${sched.name}`)
                  .onClick(() => {
                    this.store.deleteScopedSchedule(this.tab.hostId, sched.schedule_id);
                    void this.refresh();
                  })
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                if (this.expandedScheduleRunsId === sched.schedule_id) {
                  Column({ space: 4 }) {
                    if (this.expandedScheduleRuns.length === 0) {
                      Text($r('app.string.automations_no_run_history'))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                    } else {
                      ForEach(
                        this.expandedScheduleRuns,
                        (runRec: ScheduleRunRecordDto) => {
                          Row({ space: OrbitTokens.SPACE_XS }) {
                            Text(this.formatStatusLabel(runRec.status))
                              .attributeModifier(orbitTextMeta(this.statusColor(runRec.status), FontWeight.Medium))
                            Text(runRec.summary ?? runRec.error ?? runRec.started_at)
                              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                              .maxLines(1)
                              .textOverflow({ overflow: TextOverflow.Ellipsis })
                              .layoutWeight(1)
                          }
                          .width('100%')
                        },
                        (runRec: ScheduleRunRecordDto): string => `sc_run_${runRec.run_id}_${runRec.status}`
                      )
                    }
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_SM)
                  .backgroundColor(this.store.palette.recessed)
                  .borderRadius(OrbitTokens.RADIUS_SM)
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .alignItems(HorizontalAlign.Start)
            }
          },
          (sched: ScheduleSpecDto): string => `sa_sc_${sched.schedule_id}_${sched.status}_${sched.run_count}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp) })
  }

  @Builder
  private renderTimersSection() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`${this.armedTimers.length} armed · ${this.data.timers.length} total`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Blank()
          Button(this.showTimerForm ? $r('app.string.action_close') : $r('app.string.automations_new_timer'), {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showTimerForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showTimerForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.showTimerForm = !this.showTimerForm;
            })
        }
        .width('100%')
      }

      if (this.showTimerForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              TextInput({ placeholder: $r('app.string.automations_ph_seconds'), text: this.draftTimerSecs })
                .type(InputType.Number)
                .attributeModifier(
                  orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
                )
                .height(36)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .width(100)
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((v: string) => {
                  this.draftTimerSecs = v;
                  this.markFormEdited('timer');
                })
              Button(this.draftTimerCond === 'never' ? 'Unconditional' : 'Cancel on Reply', {
                type: ButtonType.Capsule,
                stateEffect: true
              })
                .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                .height(36)
                .layoutWeight(1)
                .onClick(() => {
                  this.draftTimerCond = this.draftTimerCond === 'never' ? 'any_message' : 'never';
                  this.markFormEdited('timer');
                })
            }
            .width('100%')

            TextArea({ placeholder: $r('app.string.automations_ph_timer_prompt'), text: this.draftTimerPrompt })
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.recessed)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .constraintSize({ minHeight: 54, maxHeight: 100 })
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftTimerPrompt = v;
                this.markFormEdited('timer');
              })

            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .width('100%')
              .height(34)
              .enabled(!this.submittingTimer)
              .onClick(() => {
                void this.submitTimerCreate();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        }
      }

      if (this.data.timers.length === 0) {
        ListItem() {
          this.renderEmpty('No timers', 'Conditional timers armed on this host will appear here.')
        }
      } else {
        ForEach(
          this.data.timers,
          (timer: ConditionalTimerRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(formatTimerConditionLabel(timer.condition))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .layoutWeight(1)
                  Text(this.formatRemainingTimer(timer))
                    .attributeModifier(orbitTextMono(this.statusColor(timer.state), FontWeight.Medium))
                }
                .width('100%')

                Text(timer.prompt)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                  .maxLines(3)
                  .width('100%')

                if (timer.state === 'armed') {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Blank()
                    Button('Fire Now', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                      .height(28)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.fireScopedConditionalTimerNow(this.tab.hostId, timer.timer_id).then(() => {
                          void this.refresh();
                        });
                      })
                    Button('Cancel', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
                      .height(28)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.cancelScopedConditionalTimer(this.tab.hostId, timer.timer_id).then(() => {
                          void this.refresh();
                        });
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .alignItems(HorizontalAlign.Start)
            }
          },
          (timer: ConditionalTimerRecordDto): string => `sa_tm_${timer.timer_id}_${timer.state}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp) })
  }

  @Builder
  private renderGoalsSection() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`${this.data.goals.length} goal(s)`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Blank()
          Button(this.showGoalForm ? $r('app.string.action_close') : $r('app.string.automations_new_goal'), {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showGoalForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showGoalForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.showGoalForm = !this.showGoalForm;
            })
        }
        .width('100%')
      }

      if (this.showGoalForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            TextArea({ placeholder: $r('app.string.automations_ph_goal_objective'), text: this.draftGoalObjective })
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.recessed)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .constraintSize({ minHeight: 54, maxHeight: 100 })
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftGoalObjective = v;
                this.markFormEdited('goal');
              })

            Row({ space: OrbitTokens.SPACE_XS }) {
              TextInput({ placeholder: $r('app.string.automations_ph_verify_cmd'), text: this.draftGoalVerifyCmd })
                .attributeModifier(
                  orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
                )
                .height(36)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .layoutWeight(1)
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((v: string) => {
                  this.draftGoalVerifyCmd = v;
                  this.markFormEdited('goal');
                })
              TextInput({ placeholder: $r('app.string.automations_ph_max'), text: this.draftGoalMaxIter })
                .type(InputType.Number)
                .attributeModifier(
                  orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
                )
                .height(36)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .width(56)
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((v: string) => {
                  this.draftGoalMaxIter = v;
                  this.markFormEdited('goal');
                })
            }
            .width('100%')

            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .width('100%')
              .height(34)
              .enabled(!this.submittingGoal)
              .onClick(() => {
                void this.submitGoalCreate();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        }
      }

      if (this.data.goals.length === 0) {
        ListItem() {
          this.renderEmpty('No active goals', 'Autonomous goal loops on this host will appear here.')
        }
      } else {
        ForEach(
          this.data.goals,
          (goal: GoalRunRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(goal.objective)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(2)
                    .layoutWeight(1)
                  Text(this.formatStatusLabel(goal.status))
                    .attributeModifier(orbitTextMeta(this.statusColor(goal.status), FontWeight.Medium))
                }
                .width('100%')

                Text(`Iteration ${goal.current_iteration} / ${goal.max_iterations}`)
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))

                if (goal.criteria.length > 0) {
                  Column({ space: 3 }) {
                    ForEach(
                      goal.criteria,
                      (crit: GoalCriterionDto) => {
                        Row({ space: 6 }) {
                          SymbolGlyph(
                            crit.passed ? $r('sys.symbol.checkmark_circle_fill') : $r('sys.symbol.circle')
                          )
                            .fontSize(12)
                            .fontColor([crit.passed ? this.store.palette.statusDirect : this.store.palette.fgMuted])
                          Text(crit.label)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .maxLines(1)
                            .layoutWeight(1)
                        }
                        .width('100%')
                      },
                      (crit: GoalCriterionDto): string => `sc_cr_${goal.goal_id}_${crit.id}_${crit.passed ? 1 : 0}`
                    )
                  }
                  .width('100%')
                }

                if (goal.status === 'running' || goal.status === 'verifying' || goal.status === 'paused') {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Blank()
                    if (goal.status === 'running') {
                      Button($r('app.string.automations_verify_now'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft))
                        .height(28)
                        .padding({ left: 9, right: 9 })
                        .onClick(() => {
                          this.store.controlScopedGoalRun(this.tab.hostId, goal.goal_id, 'verify_now').then(() => {
                            void this.refresh();
                          });
                        })
                    }
                    Button(goal.status === 'paused' ? 'Resume' : 'Pause', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                      .height(28)
                      .padding({ left: 9, right: 9 })
                      .onClick(() => {
                        this.store.controlScopedGoalRun(
                          this.tab.hostId,
                          goal.goal_id,
                          goal.status === 'paused' ? 'resume' : 'pause'
                        ).then(() => {
                          void this.refresh();
                        });
                      })
                    Button('Stop', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
                      .height(28)
                      .padding({ left: 9, right: 9 })
                      .onClick(() => {
                        this.store.controlScopedGoalRun(this.tab.hostId, goal.goal_id, 'cancel').then(() => {
                          void this.refresh();
                        });
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .alignItems(HorizontalAlign.Start)
            }
          },
          (goal: GoalRunRecordDto): string => `sa_gl_${goal.goal_id}_${goal.status}_${goal.current_iteration}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp) })
  }

  @Builder
  private renderMonitorsSection() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`${this.data.monitors.length} monitor(s)`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Blank()
          Button(this.showMonitorForm ? $r('app.string.action_close') : $r('app.string.automations_new_monitor'), {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showMonitorForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showMonitorForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.showMonitorForm = !this.showMonitorForm;
            })
        }
        .width('100%')
      }

      if (this.showMonitorForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            TextInput({ placeholder: $r('app.string.automations_ph_monitor_name'), text: this.draftMonitorName })
              .attributeModifier(
                orbitInputSubhead(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftMonitorName = v;
                this.markFormEdited('monitor');
              })

            Row({ space: OrbitTokens.SPACE_XS }) {
              Button('Command', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(
                    this.draftMonitorKind === 'command_stream' ? this.store.palette.onAccent : this.store.palette.fgSecondary,
                    this.draftMonitorKind === 'command_stream' ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
                  )
                )
                .height(28)
                .layoutWeight(1)
                .onClick(() => {
                  this.draftMonitorKind = 'command_stream';
                  this.markFormEdited('monitor');
                })
              Button('Log File', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(
                    this.draftMonitorKind === 'log_file_tail' ? this.store.palette.onAccent : this.store.palette.fgSecondary,
                    this.draftMonitorKind === 'log_file_tail' ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
                  )
                )
                .height(28)
                .layoutWeight(1)
                .onClick(() => {
                  this.draftMonitorKind = 'log_file_tail';
                  this.markFormEdited('monitor');
                })
              Button('HTTP', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(
                    this.draftMonitorKind === 'http_health' ? this.store.palette.onAccent : this.store.palette.fgSecondary,
                    this.draftMonitorKind === 'http_health' ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
                  )
                )
                .height(28)
                .layoutWeight(1)
                .onClick(() => {
                  this.draftMonitorKind = 'http_health';
                  this.markFormEdited('monitor');
                })
            }
            .width('100%')

            TextInput({
              placeholder: this.draftMonitorKind === 'http_health'
                ? 'https://service.local/health'
                : (this.draftMonitorKind === 'log_file_tail' ? '/var/log/app.log' : 'tail -f build.log'),
              text: this.draftMonitorTarget
            })
              .attributeModifier(
                orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftMonitorTarget = v;
                this.markFormEdited('monitor');
              })

            if (this.draftMonitorKind !== 'http_health') {
              TextInput({ placeholder: $r('app.string.automations_ph_regex'), text: this.draftMonitorPattern })
                .attributeModifier(
                  orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
                )
                .height(36)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .width('100%')
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((v: string) => {
                  this.draftMonitorPattern = v;
                  this.markFormEdited('monitor');
                })
            }

            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .width('100%')
              .height(34)
              .enabled(!this.submittingMonitor)
              .onClick(() => {
                void this.submitMonitorCreate();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        }
      }

      if (this.data.monitors.length === 0) {
        ListItem() {
          this.renderEmpty('No active monitors', 'Watch live command streams, logs, or HTTP health endpoints.')
        }
      } else {
        ForEach(
          this.data.monitors,
          (mon: MonitorWatchRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(mon.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .layoutWeight(1)
                  Text(`${this.formatStatusLabel(mon.status)} · ${mon.match_count}`)
                    .attributeModifier(orbitTextMeta(this.statusColor(mon.status)))
                }
                .width('100%')

                if (mon.last_event_preview !== undefined && mon.last_event_preview !== null && mon.last_event_preview.length > 0) {
                  Text(mon.last_event_preview)
                    .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                    .maxLines(2)
                    .width('100%')
                }

                if (mon.status === 'active' || mon.status === 'triggered' || mon.status === 'paused') {
                  Row() {
                    Blank()
                    Button('Stop', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
                      .height(28)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.stopScopedMonitorWatch(this.tab.hostId, mon.monitor_id).then(() => {
                          void this.refresh();
                        });
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .alignItems(HorizontalAlign.Start)
            }
          },
          (mon: MonitorWatchRecordDto): string => `sa_mon_${mon.monitor_id}_${mon.status}_${mon.match_count}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp) })
  }

  @Builder
  private renderWorkflowsSection() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`${this.data.workflows.length} run(s)`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Blank()
          Button(this.showWorkflowForm ? $r('app.string.action_close') : $r('app.string.automations_new_workflow'), {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showWorkflowForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showWorkflowForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.showWorkflowForm = !this.showWorkflowForm;
            })
        }
        .width('100%')
      }

      if (this.showWorkflowForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            TextInput({ placeholder: $r('app.string.automations_ph_workflow_name'), text: this.draftWorkflowName })
              .attributeModifier(
                orbitInputSubhead(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftWorkflowName = v;
                this.markFormEdited('workflow');
              })

            TextArea({ placeholder: $r('app.string.automations_ph_workflow_steps'), text: this.draftWorkflowPrompt })
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.recessed)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .constraintSize({ minHeight: 54, maxHeight: 100 })
              .width('100%')
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((v: string) => {
                this.draftWorkflowPrompt = v;
                this.markFormEdited('workflow');
              })

            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .width('100%')
              .height(34)
              .enabled(!this.submittingWorkflow)
              .onClick(() => {
                void this.submitWorkflowCreate();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        }
      }

      if (this.data.workflows.length === 0) {
        ListItem() {
          this.renderEmpty('No workflow runs', 'Batch fan-out, DAG pipelines, and arenas on this host appear here.')
        }
      } else {
        ForEach(
          this.data.workflows,
          (run: WorkflowRunRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Text(run.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .layoutWeight(1)
                  Text(formatWorkflowKindLabel(run.kind))
                    .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                  Text(this.formatStatusLabel(run.status))
                    .attributeModifier(orbitTextMeta(this.statusColor(run.status), FontWeight.Medium))
                }
                .width('100%')

                if (run.steps.length > 0) {
                  Column({ space: 4 }) {
                    ForEach(
                      run.steps,
                      (step: WorkflowStepRecordDto) => {
                        Row({ space: OrbitTokens.SPACE_XS }) {
                          Row()
                            .width(6)
                            .height(6)
                            .borderRadius(3)
                            .backgroundColor(this.statusColor(step.status))
                          Text(step.name)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                            .maxLines(1)
                            .layoutWeight(1)
                          Text(formatProviderLabel(step.provider))
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        }
                        .width('100%')
                      },
                      (step: WorkflowStepRecordDto): string => `sa_wfs_${run.run_id}_${step.step_id}_${step.status}`
                    )
                  }
                  .width('100%')
                }

                if (run.status === 'running' || run.status === 'pending') {
                  Row() {
                    Blank()
                    Button('Cancel Run', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
                      .height(28)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.cancelScopedWorkflowRun(this.tab.hostId, run.run_id).then(() => {
                          void this.refresh();
                        });
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .alignItems(HorizontalAlign.Start)
            }
          },
          (run: WorkflowRunRecordDto): string => `sa_wf_${run.run_id}_${run.status}_${run.updated_at_ms}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp) })
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.automations_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          Text(
            `${this.data.schedules.length} sched · ${this.data.timers.length} timer · ${this.data.goals.length} goal · ${this.data.monitors.length} mon · ${this.data.workflows.length} wf`
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.loading) {
          LoadingProgress()
            .width(16)
            .height(16)
            .color(this.store.palette.accentPrimary)
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(30)
        .height(30)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.automations_refresh_a11y'))
        .onClick(() => {
          void this.refresh();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          ForEach(
            SCOPED_AUTOMATION_TABS,
            (tSpec: ScopedAutomationTabSpec) => {
              Button(tSpec.label, { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(
                    this.activeTabIndex === tSpec.idx
                      ? this.store.palette.onAccent
                      : this.store.palette.fgSecondary,
                    this.activeTabIndex === tSpec.idx
                      ? this.store.palette.accentPrimaryFill
                      : this.store.palette.listCard
                  )
                )
                .height(30)
                .padding({ left: 10, right: 10 })
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .onClick(() => {
                  this.activeTabIndex = tSpec.idx;
                  this.store.triggerHapticPreset('selection');
                })
            },
            (tSpec: ScopedAutomationTabSpec): string => `sa_tab_${tSpec.idx}`
          )
        }
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      if (this.activeTabIndex === 0) {
        this.renderSchedulesSection()
      } else if (this.activeTabIndex === 1) {
        this.renderTimersSection()
      } else if (this.activeTabIndex === 2) {
        this.renderGoalsSection()
      } else if (this.activeTabIndex === 3) {
        this.renderMonitorsSection()
      } else {
        this.renderWorkflowsSection()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
