import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitPersonalAgentStore } from '../../model/stores/OrbitPersonalAgentStore';
import {
  AcpAgentAvailabilityDto,
  AcpAgentSpecDto,
  AcpAgentTestResultDto,
  GuardianPolicyConfigDto,
  OrbitMeshPeerRow,
  PermissionRuleRecordDto,
  PersonalRouterConfigDto,
  ProviderModelCatalogDto,
  ProviderModelSpecDto,
  ProviderQuotaDto,
  QuotaDepletionForecastDto,
  QuotaWindowDto,
  RouterHostCapabilitiesDto,
  UpsertPermissionRuleInputDto,
  UsageDailyPointDto,
  UsageInsightsSnapshotDto,
  UsageModelBreakdownDto,
  UsageProjectBreakdownDto,
  basenameOfPath,
  formatProviderLabel,
  formatQuotaResetCountdown,
  normalizeModeSlug
} from '../../model/OrbitTypes';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSectionHeader, dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { OrbitProviderBrandIcon } from '../composer/ModelPickerSheet';
import {
  ORBIT_USAGE_PERIOD_DAYS,
  OrbitSettingsControlBinding,
  OrbitSettingsRequestGuard,
  createSettingsControlBinding,
  isSettingsControlBindingActive
} from './OrbitSettingsNavModel';

const ACCOUNT_MODES: string[] = ['default', 'auto', 'accept_edits', 'full_access'];
const GUARDIAN_RISK_LEVELS: string[] = ['low', 'medium'];
const RULE_TOOL_KINDS: string[] = ['any', 'shell_command', 'file_write', 'mcp_tool', 'network'];
const RULE_DECISIONS: string[] = ['allow', 'deny', 'ask'];

interface AgentsModelOptionBinding {
  spec: ProviderModelSpecDto;
  control: OrbitSettingsControlBinding;
}

interface AgentsEffortOptionBinding {
  effort: string;
  control: OrbitSettingsControlBinding;
}

interface AgentsProviderCatalogBinding {
  catalog: ProviderModelCatalogDto;
  control: OrbitSettingsControlBinding;
}

interface AgentsAcpRowBinding {
  item: AcpAgentAvailabilityDto;
  control: OrbitSettingsControlBinding;
}

interface PermissionRuleRowBinding {
  rule: PermissionRuleRecordDto;
  control: OrbitSettingsControlBinding;
}

interface GuardianPolicyBinding {
  policy: GuardianPolicyConfigDto;
  control: OrbitSettingsControlBinding;
}

interface PersonalAgentHostRowBinding {
  cand: RouterHostCapabilitiesDto;
  control: OrbitSettingsControlBinding;
}

interface PersonalAgentMeshRowBinding {
  row: OrbitMeshPeerRow;
  control: OrbitSettingsControlBinding;
}

interface PermStringOptionBinding {
  value: string;
  control: OrbitSettingsControlBinding;
}

@ComponentV2
export struct OrbitSettingsPersonalAgentPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private personalAgentStore: OrbitPersonalAgentStore = orbitRuntime().personalAgentStore;
  @Local private selectedRouterHostId: string = '';
  @Local private draftEnabled: boolean = false;
  @Local private draftScope: string = 'global_fleet';
  @Local private draftPreReview: string = 'disabled';
  @Local private draftShareLocalMemory: boolean = true;
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private paControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isPaControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      binding.scopeKey
    );
  }

  @Computed
  get boundRouterHosts(): PersonalAgentHostRowBinding[] {
    const control = createSettingsControlBinding(
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.visibleEpoch,
      'pa_cand'
    );
    const out: PersonalAgentHostRowBinding[] = [];
    for (let i = 0; i < this.store.routerHosts.length; i++) {
      out.push({ cand: this.store.routerHosts[i], control: control });
    }
    return out;
  }

  @Computed
  get boundMeshPeers(): PersonalAgentMeshRowBinding[] {
    const control = createSettingsControlBinding(
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.visibleEpoch,
      'pa_mesh'
    );
    const out: PersonalAgentMeshRowBinding[] = [];
    for (let i = 0; i < this.store.fleetMeshPeers.length; i++) {
      out.push({ row: this.store.fleetMeshPeers[i], control: control });
    }
    return out;
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.syncDraftFromStore();
    this.store.refreshRouterAndMesh();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostContextChanged(_mon: IMonitor): void {
    this.syncDraftFromStore();
    if (this.pageMounted && this.pageVisible) {
      this.store.refreshRouterAndMesh();
    }
  }

  @Monitor('personalAgentStore.activeRouterStatus')
  onRouterStatusUpdated(_mon: IMonitor): void {
    if (!this.pageMounted) {
      return;
    }
    this.syncDraftFromStore();
  }

  private refreshRouterAndMesh(binding: OrbitSettingsControlBinding): void {
    if (!this.isPaControlActive(binding)) {
      return;
    }
    this.store.triggerHaptic('selection');
    this.store.refreshRouterAndMesh();
  }

  private syncDraftFromStore(): void {
    const snap = this.store.activeRouterStatus;
    if (snap !== null) {
      this.selectedRouterHostId = snap.host_capabilities.host_id;
      this.draftEnabled = snap.config.enabled;
      this.draftScope = snap.config.scope;
      this.draftPreReview = snap.config.pre_review_mode;
      this.draftShareLocalMemory = snap.config.share_host_hermes_memory;
    } else if (this.store.primaryRouterHostId.length > 0) {
      this.selectedRouterHostId = this.store.primaryRouterHostId;
    } else if (this.resolvedHostId().length > 0) {
      this.selectedRouterHostId = this.resolvedHostId();
    }
  }

  private buildUpdatedConfig(): PersonalRouterConfigDto {
    const existing = this.store.activeRouterStatus?.config;
    return {
      enabled: this.draftEnabled,
      router_agent_id: existing !== undefined ? existing.router_agent_id : null,
      provider: existing !== undefined ? existing.provider : this.store.composerProvider,
      acp_agent_id: existing !== undefined ? existing.acp_agent_id : null,
      model: existing !== undefined ? existing.model : null,
      effort: existing !== undefined ? existing.effort : null,
      cwd: existing !== undefined ? existing.cwd : null,
      scope: this.draftScope,
      pre_review_mode: this.draftPreReview,
      share_host_hermes_memory: this.draftShareLocalMemory,
      soul_prompt_override: existing !== undefined ? existing.soul_prompt_override : null,
      external_webhook_url: existing !== undefined ? existing.external_webhook_url : null,
      has_external_webhook_secret: existing !== undefined ? existing.has_external_webhook_secret : false
    };
  }

  private peerStatusColor(status: string): ResourceColor {
    const s = status.toLowerCase();
    if (s === 'connected' || s === 'online' || s === 'online_direct') {
      return this.store.palette.statusDirect;
    }
    if (s === 'online_relay' || s === 'relay') {
      return this.store.palette.statusRelay;
    }
    if (s === 'connecting') {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.fgMuted;
  }

  build() {
    NavDestination() {
      Scroll() {
        Column({ space: OrbitTokens.SPACE_MD }) {
          OrbitSectionHeader({ title: $r('app.string.router_section_hosts'), isFirst: true })

          if (this.store.routerHosts.length === 0) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.settings_pa_no_hosts'))
                .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              ForEach(
                this.paControlBindings('pa_refresh'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding({ left: 14, right: 14 })
                    .accessibilityText($r('app.string.action_refresh'))
                    .onClick(() => {
                      this.refreshRouterAndMesh(ctrl);
                    })
                },
                (ctrl: OrbitSettingsControlBinding) => `pa_refresh_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          } else {
            Column({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                this.boundRouterHosts,
                (entry: PersonalAgentHostRowBinding) => {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Row()
                        .width(8)
                        .height(8)
                        .borderRadius(4)
                        .backgroundColor(
                          entry.cand.online ? this.store.palette.statusDirect : this.store.palette.fgMuted
                        )

                      Column({ space: 2 }) {
                        Row({ space: 6 }) {
                          Text(entry.cand.host_name)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                          if (entry.cand.hermes_installed) {
                            Text($r('app.string.settings_pa_memory_capable'))
                              .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
                              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                              .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                              .backgroundColor(this.store.palette.accentPrimarySoft)
                              .borderRadius(999)
                          }
                        }
                        Text(
                          entry.cand.recommendation_reason !== undefined && entry.cand.recommendation_reason !== null
                            ? entry.cand.recommendation_reason
                            : `${entry.cand.recommended_score} · ${entry.cand.available_providers.join(', ')}`
                        )
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      if (this.selectedRouterHostId === entry.cand.host_id) {
                        SymbolGlyph($r('sys.symbol.checkmark_circle_fill'))
                          .fontSize(16)
                          .fontColor([this.store.palette.accentPrimary])
                      }
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .accessibilityText(entry.cand.host_name)
                  .onClick(() => {
                    if (!this.isPaControlActive(entry.control)) {
                      return;
                    }
                    this.selectedRouterHostId = entry.cand.host_id;
                    this.store.triggerHaptic('selection');
                    this.store.loadRouterStatusForHost(entry.cand.host_id);
                  })
                },
                (entry: PersonalAgentHostRowBinding) => `pa_cand_${entry.control.renderKey}_${entry.cand.host_id}`
              )
            }
            .width('100%')
          }

          if (this.selectedRouterHostId.length > 0) {
            OrbitSectionHeader({ title: $r('app.string.router_section_config') })

            ForEach(
              this.paControlBindings('pa_config'),
              (ctrl: OrbitSettingsControlBinding) => {
                Column({ space: OrbitTokens.SPACE_MD }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text($r('app.string.router_enable_toggle'))
                      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      .layoutWeight(1)
                    Toggle({ type: ToggleType.Switch, isOn: this.draftEnabled })
                      .selectedColor(this.store.palette.accentPrimaryFill)
                      .accessibilityText($r('app.string.router_enable_toggle'))
                      .onChange((val: boolean) => {
                        if (!this.isPaControlActive(ctrl)) {
                          return;
                        }
                        this.draftEnabled = val;
                      })
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)

                  Column({ space: OrbitTokens.SPACE_XS }) {
                    Text($r('app.string.settings_pa_scope_label'))
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Button($r('app.string.settings_pa_scope_fleet'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.draftScope === 'global_fleet'
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.draftScope === 'global_fleet'
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_pa_scope_fleet'))
                        .onClick(() => {
                          if (!this.isPaControlActive(ctrl)) {
                            return;
                          }
                          this.draftScope = 'global_fleet';
                          this.store.triggerHaptic('selection');
                        })
                      Button($r('app.string.settings_pa_scope_local'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.draftScope === 'local_host_only'
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.draftScope === 'local_host_only'
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_pa_scope_local'))
                        .onClick(() => {
                          if (!this.isPaControlActive(ctrl)) {
                            return;
                          }
                          this.draftScope = 'local_host_only';
                          this.store.triggerHaptic('selection');
                        })
                    }
                    .width('100%')
                  }
                  .width('100%')
                  .alignItems(HorizontalAlign.Start)

                  Column({ space: OrbitTokens.SPACE_XS }) {
                    Text($r('app.string.settings_pa_prereview_label'))
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Button($r('app.string.settings_state_off'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.draftPreReview === 'disabled'
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.draftPreReview === 'disabled'
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_state_off'))
                        .onClick(() => {
                          if (!this.isPaControlActive(ctrl)) {
                            return;
                          }
                          this.draftPreReview = 'disabled';
                          this.store.triggerHaptic('selection');
                        })
                      Button($r('app.string.settings_pa_prereview_medium'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.draftPreReview === 'medium_risk_only'
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.draftPreReview === 'medium_risk_only'
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_pa_prereview_medium'))
                        .onClick(() => {
                          if (!this.isPaControlActive(ctrl)) {
                            return;
                          }
                          this.draftPreReview = 'medium_risk_only';
                          this.store.triggerHaptic('selection');
                        })
                      Button($r('app.string.settings_pa_prereview_assist'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.draftPreReview === 'assist_only'
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.draftPreReview === 'assist_only'
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_pa_prereview_assist'))
                        .onClick(() => {
                          if (!this.isPaControlActive(ctrl)) {
                            return;
                          }
                          this.draftPreReview = 'assist_only';
                          this.store.triggerHaptic('selection');
                        })
                    }
                    .width('100%')
                  }
                  .width('100%')
                  .alignItems(HorizontalAlign.Start)

                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text($r('app.string.router_share_hermes'))
                      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      .layoutWeight(1)
                    Toggle({ type: ToggleType.Switch, isOn: this.draftShareLocalMemory })
                      .selectedColor(this.store.palette.accentPrimaryFill)
                      .accessibilityText($r('app.string.router_share_hermes'))
                      .onChange((val: boolean) => {
                        if (!this.isPaControlActive(ctrl)) {
                          return;
                        }
                        this.draftShareLocalMemory = val;
                      })
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)

                  Button($r('app.string.router_apply'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                    )
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .enabled(!this.store.routerMeshBusy)
                    .accessibilityText($r('app.string.router_apply'))
                    .onClick(() => {
                      if (!this.isPaControlActive(ctrl) || this.selectedRouterHostId.length === 0) {
                        return;
                      }
                      this.store.triggerHaptic('success');
                      this.store.saveRouterConfig(
                        this.selectedRouterHostId,
                        this.buildUpdatedConfig(),
                        this.draftEnabled
                      );
                    })
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              },
              (ctrl: OrbitSettingsControlBinding) => `pa_cfg_${ctrl.renderKey}`
            )
          }

          OrbitSectionHeader({ title: $r('app.string.router_section_mesh') })

          if (this.store.fleetMeshPeers.length === 0) {
            Column() {
              Text($r('app.string.settings_pa_mesh_empty'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          } else {
            Column({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                this.boundMeshPeers,
                (entry: PersonalAgentMeshRowBinding) => {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Row()
                      .width(8)
                      .height(8)
                      .borderRadius(4)
                      .backgroundColor(this.peerStatusColor(entry.row.peer.status))

                    Column({ space: 2 }) {
                      Text(`${entry.row.owner_host_name} → ${entry.row.peer.label}`)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      Text(`${entry.row.peer.status}${entry.row.peer.hostname ? ` · ${entry.row.peer.hostname}` : ''}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .maxLines(1)
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    Toggle({ type: ToggleType.Switch, isOn: entry.row.peer.enabled })
                      .selectedColor(this.store.palette.accentPrimaryFill)
                      .accessibilityText(entry.row.peer.label)
                      .onChange((nextOn: boolean) => {
                        if (!this.isPaControlActive(entry.control)) {
                          return;
                        }
                        if (nextOn !== entry.row.peer.enabled) {
                          this.store.triggerHaptic('selection');
                          this.store.toggleMeshPeerEnabled(
                            entry.row.owner_host_id,
                            entry.row.peer.registration_id,
                            entry.row.peer.label,
                            nextOn
                          );
                        }
                      })
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .alignItems(VerticalAlign.Center)
                },
                (entry: PersonalAgentMeshRowBinding) =>
                  `pa_mesh_${entry.control.renderKey}_${entry.row.owner_host_id}_${entry.row.peer.registration_id}`
              )
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_XS,
          bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
        })
      }
      .width('100%')
      .height('100%')
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.nav_personal_router'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.syncDraftFromStore();
        this.store.refreshRouterAndMesh();
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsAgentsModelsPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private acpAgents: AcpAgentAvailabilityDto[] = [];
  @Local private loadingRegistry: boolean = false;
  @Local private testingAgentId: string = '';
  @Local private lastTestResult: AcpAgentTestResultDto | null = null;
  @Local private errorText: string = '';
  @Local private showAddCustomAcp: boolean = false;
  @Local private customAcpId: string = '';
  @Local private customAcpName: string = '';
  @Local private customAcpCommand: string = '';
  @Local private customAcpArgs: string = '';
  @Local private savingCustom: boolean = false;
  @Local private expandedPicker: string = ''; // '' | 'model' | 'effort'
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private currentAgentsActionBinding(scopeKey: string): OrbitSettingsControlBinding {
    return createSettingsControlBinding(
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.visibleEpoch,
      scopeKey
    );
  }

  private isAgentsActionActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      binding.scopeKey
    );
  }

  @Computed
  get targetDefaultProvider(): string {
    const _rev = this.store.hostDefaultPrefsRevision;
    return _rev >= 0 ? this.store.getHostDefaultProvider(this.resolvedHostId()) : this.store.composerProvider;
  }

  @Computed
  get targetDefaultModel(): string {
    const _rev = this.store.hostDefaultPrefsRevision;
    return _rev >= 0 ? this.store.getHostDefaultModel(this.resolvedHostId()) : this.store.composerModel;
  }

  @Computed
  get targetDefaultEffort(): string {
    const _rev = this.store.hostDefaultPrefsRevision;
    return _rev >= 0 ? this.store.getHostDefaultEffort(this.resolvedHostId()) : this.store.composerEffort;
  }

  @Computed
  get providerCatalogs(): ProviderModelCatalogDto[] {
    const target = this.resolvedHostId();
    const detail = target.length > 0 ? this.store.hostDetailFor(target) : null;
    if (detail !== null && detail.provider_catalogs.length > 0) {
      return detail.provider_catalogs;
    }
    return [];
  }

  @Computed
  get activeProviderModels(): ProviderModelSpecDto[] {
    return this.store.modelsForHostProvider(this.resolvedHostId(), this.targetDefaultProvider);
  }

  @Computed
  get boundProviderModels(): AgentsModelOptionBinding[] {
    const control = this.currentAgentsActionBinding('agent_model');
    const models = this.activeProviderModels;
    const out: AgentsModelOptionBinding[] = [];
    for (let i = 0; i < models.length; i++) {
      out.push({ spec: models[i], control: control });
    }
    return out;
  }

  @Computed
  get boundProviderCatalogs(): AgentsProviderCatalogBinding[] {
    const control = this.currentAgentsActionBinding('agent_provider');
    const cats = this.providerCatalogs;
    const out: AgentsProviderCatalogBinding[] = [];
    for (let i = 0; i < cats.length; i++) {
      out.push({ catalog: cats[i], control: control });
    }
    return out;
  }

  @Computed
  get boundAcpAgents(): AgentsAcpRowBinding[] {
    const control = this.currentAgentsActionBinding('acp');
    const out: AgentsAcpRowBinding[] = [];
    for (let i = 0; i < this.acpAgents.length; i++) {
      out.push({ item: this.acpAgents[i], control: control });
    }
    return out;
  }

  @Computed
  get availableEfforts(): string[] {
    const models = this.activeProviderModels;
    const selectedId = this.targetDefaultModel.trim();
    let matched: ProviderModelSpecDto | null = null;
    for (let i = 0; i < models.length; i++) {
      if (models[i].id === selectedId) {
        matched = models[i];
        break;
      }
    }
    if (matched === null && models.length > 0) {
      matched = models[0];
    }
    if (matched !== null && matched.thinking_options.length > 0) {
      const efforts: string[] = [];
      for (let i = 0; i < matched.thinking_options.length; i++) {
        const eff = matched.thinking_options[i].effort.trim();
        if (eff.length > 0 && !efforts.includes(eff)) {
          efforts.push(eff);
        }
      }
      if (efforts.length > 0) {
        return efforts;
      }
    }
    return ['low', 'medium', 'high', 'xhigh'];
  }

  @Computed
  get boundEfforts(): AgentsEffortOptionBinding[] {
    const control = this.currentAgentsActionBinding('agent_effort');
    const efforts = this.availableEfforts;
    const out: AgentsEffortOptionBinding[] = [];
    for (let i = 0; i < efforts.length; i++) {
      out.push({ effort: efforts[i], control: control });
    }
    return out;
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadLocalAcpAgents();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.loadingRegistry = false;
    this.testingAgentId = '';
    this.savingCustom = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), 'acp');
    this.acpAgents = [];
    this.lastTestResult = null;
    this.testingAgentId = '';
    this.loadingRegistry = false;
    this.savingCustom = false;
    this.errorText = '';
    if (this.pageMounted && this.pageVisible) {
      this.loadLocalAcpAgents();
    }
  }

  private loadLocalAcpAgents(): void {
    if (!this.pageMounted || !this.pageVisible) {
      return;
    }
    const target = this.resolvedHostId();
    if (target.length === 0) {
      this.acpAgents = [];
      return;
    }
    const res = this.store.listAcpAgentsScoped(target);
    if (res.ok && res.data !== null) {
      this.acpAgents = res.data;
    } else {
      this.errorText = res.errorDetail;
    }
  }

  private refreshRegistry(binding: OrbitSettingsControlBinding): void {
    if (!this.isAgentsActionActive(binding) || binding.hostId.length === 0 || this.loadingRegistry) {
      return;
    }
    const target = binding.hostId;
    const token = this.guard.beginLoad(target, 'acp_refresh', binding.hostSelectionEpoch, binding.rootEpoch);
    this.loadingRegistry = true;
    this.errorText = '';
    this.store.refreshAcpRegistryScoped(target, null).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loadingRegistry = false;
      if (res.ok && res.data !== null) {
        this.acpAgents = res.data;
        this.store.triggerHapticPreset('success');
      } else {
        this.errorText = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private testAcpAgent(agentId: string, binding: OrbitSettingsControlBinding): void {
    if (!this.isAgentsActionActive(binding) || binding.hostId.length === 0 || this.testingAgentId.length > 0) {
      return;
    }
    const token = this.guard.beginItemWrite(
      binding.hostId,
      'acp',
      `test:${agentId}`,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.testingAgentId = agentId;
    this.errorText = '';
    this.store.testAcpAgentScoped(binding.hostId, agentId, null).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guard.isItemWriteCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.testingAgentId = '';
      if (res.ok && res.data !== null) {
        this.lastTestResult = res.data;
        this.store.triggerHapticPreset(res.data.ok ? 'success' : 'warning');
      } else {
        this.errorText = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private saveCustomAcpAgent(binding: OrbitSettingsControlBinding): void {
    if (!this.isAgentsActionActive(binding)) {
      return;
    }
    const target = binding.hostId;
    const id = this.customAcpId.trim();
    const name = this.customAcpName.trim();
    const cmd = this.customAcpCommand.trim();
    if (target.length === 0 || id.length === 0 || name.length === 0 || cmd.length === 0 || this.savingCustom) {
      return;
    }
    const args = this.customAcpArgs.trim().length > 0
      ? this.customAcpArgs.trim().split(/\s+/).filter((s: string): boolean => s.length > 0)
      : [];
    const spec: AcpAgentSpecDto = {
      id: id,
      name: name,
      description: '',
      command: cmd,
      args: args,
      env: {},
      source: 'custom',
      enabled: true
    };
    const token = this.guard.beginItemWrite(
      target,
      'acp',
      `save:${id}`,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.savingCustom = true;
    this.errorText = '';
    this.store.upsertCustomAcpAgentScoped(target, spec).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guard.isItemWriteCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.savingCustom = false;
      if (res.ok && res.data !== null) {
        this.acpAgents = res.data;
        this.showAddCustomAcp = false;
        this.customAcpId = '';
        this.customAcpName = '';
        this.customAcpCommand = '';
        this.customAcpArgs = '';
        this.store.triggerHapticPreset('success');
      } else {
        this.errorText = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private deleteCustomAcpAgent(agentId: string, binding: OrbitSettingsControlBinding): void {
    if (!this.isAgentsActionActive(binding) || binding.hostId.length === 0) {
      return;
    }
    const token = this.guard.beginItemWrite(
      binding.hostId,
      'acp',
      `del:${agentId}`,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.store.deleteCustomAcpAgentScoped(binding.hostId, agentId).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guard.isItemWriteCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      if (res.ok && res.data !== null) {
        this.acpAgents = res.data;
        this.store.triggerHapticPreset('warning');
      } else {
        this.errorText = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        // Default Composer Provider / Model / Effort summary
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_agents_defaults_section'), isFirst: true })
        }

        ListItemGroup() {
          ListItem() {
            Row() {
              Text($r('app.string.settings_agents_provider_label'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Blank()
              Text(formatProviderLabel(this.targetDefaultProvider))
                .attributeModifier(orbitTextMono(this.store.palette.accentPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                [this.currentAgentsActionBinding('agent_expand_model')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Text($r('app.string.settings_agents_model_label'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      Blank()
                      Text(
                        this.targetDefaultModel.length > 0
                          ? this.targetDefaultModel
                          : $r('app.string.common_default')
                      )
                        .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      SymbolGlyph(
                        this.expandedPicker === 'model'
                          ? $r('sys.symbol.chevron_down')
                          : $r('sys.symbol.chevron_right')
                      )
                        .fontSize(12)
                        .fontColor([this.store.palette.fgMuted])
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  .backgroundColor(Color.Transparent)
                  .accessibilityText($r('app.string.settings_agents_select_model_hint'))
                  .onClick(() => {
                    if (!this.isAgentsActionActive(ctrl)) {
                      return;
                    }
                    this.expandedPicker = this.expandedPicker === 'model' ? '' : 'model';
                    this.store.triggerHapticPreset('selection');
                  })
                },
                (ctrl: OrbitSettingsControlBinding) => `ag_exp_m_${ctrl.renderKey}`
              )

              if (this.expandedPicker === 'model') {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  if (this.activeProviderModels.length === 0) {
                    Text($r('app.string.settings_agents_no_models'))
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, bottom: OrbitTokens.SPACE_SM })
                  } else {
                    ForEach(
                      this.boundProviderModels,
                      (row: AgentsModelOptionBinding) => {
                        Button({ type: ButtonType.Normal, stateEffect: true }) {
                          Row({ space: OrbitTokens.SPACE_SM }) {
                            Text(row.spec.label || row.spec.id)
                              .attributeModifier(
                                orbitTextMeta(
                                  this.targetDefaultModel === row.spec.id
                                    ? this.store.palette.accentPrimary
                                    : this.store.palette.fgPrimary,
                                  this.targetDefaultModel === row.spec.id ? FontWeight.Medium : FontWeight.Regular
                                )
                              )
                              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                              .maxLines(1)
                              .textOverflow({ overflow: TextOverflow.Ellipsis })
                              .layoutWeight(1)
                            if (this.targetDefaultModel === row.spec.id) {
                              SymbolGlyph($r('sys.symbol.checkmark'))
                                .fontSize(13)
                                .fontColor([this.store.palette.accentPrimary])
                            }
                          }
                          .width('100%')
                          .alignItems(VerticalAlign.Center)
                        }
                        .width('100%')
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                        .backgroundColor(
                          this.targetDefaultModel === row.spec.id
                            ? this.store.palette.subtleSurface
                            : Color.Transparent
                        )
                        .borderRadius(OrbitTokens.RADIUS_SM)
                        .accessibilityText(row.spec.label || row.spec.id)
                        .onClick(() => {
                          if (!this.isAgentsActionActive(row.control)) {
                            return;
                          }
                          this.store.setHostDefaultModel(row.control.hostId, row.spec.id);
                        })
                      },
                      (row: AgentsModelOptionBinding) => `mopt_${row.control.renderKey}_${row.spec.id}`
                    )
                  }
                }
                .width('100%')
                .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_SM })
              }
            }
            .width('100%')
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                [this.currentAgentsActionBinding('agent_expand_effort')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Text($r('app.string.settings_agents_effort_label'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      Blank()
                      Text(
                        this.targetDefaultEffort.length > 0
                          ? this.targetDefaultEffort
                          : $r('app.string.common_default')
                      )
                        .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      SymbolGlyph(
                        this.expandedPicker === 'effort'
                          ? $r('sys.symbol.chevron_down')
                          : $r('sys.symbol.chevron_right')
                      )
                        .fontSize(12)
                        .fontColor([this.store.palette.fgMuted])
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  .backgroundColor(Color.Transparent)
                  .accessibilityText($r('app.string.settings_agents_select_effort_hint'))
                  .onClick(() => {
                    if (!this.isAgentsActionActive(ctrl)) {
                      return;
                    }
                    this.expandedPicker = this.expandedPicker === 'effort' ? '' : 'effort';
                    this.store.triggerHapticPreset('selection');
                  })
                },
                (ctrl: OrbitSettingsControlBinding) => `ag_exp_e_${ctrl.renderKey}`
              )

              if (this.expandedPicker === 'effort') {
                Scroll() {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    ForEach(
                      this.boundEfforts,
                      (row: AgentsEffortOptionBinding) => {
                        Button(row.effort, { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(
                              this.targetDefaultEffort === row.effort
                                ? this.store.palette.onAccent
                                : this.store.palette.fgSecondary,
                              this.targetDefaultEffort === row.effort
                                ? this.store.palette.accentPrimaryFill
                                : this.store.palette.subtleSurface
                            )
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                          .accessibilityText(row.effort)
                          .onClick(() => {
                            if (!this.isAgentsActionActive(row.control)) {
                              return;
                            }
                            this.store.setHostDefaultEffort(row.control.hostId, row.effort);
                          })
                      },
                      (row: AgentsEffortOptionBinding) => `eff_${row.control.renderKey}_${row.effort}`
                    )
                  }
                }
                .scrollable(ScrollDirection.Horizontal)
                .scrollBar(BarState.Off)
                .width('100%')
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, bottom: OrbitTokens.SPACE_SM })
              }
            }
            .width('100%')
          }
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: OrbitTokens.SPACE_LG,
          endMargin: OrbitTokens.SPACE_LG
        })

        // Installed Providers Catalog on Host
        ListItem() {
          Row() {
            OrbitSectionHeader({ title: $r('app.string.settings_agents_providers_section') })
            Blank()
            if (this.resolvedHostId().length > 0) {
              ForEach(
                [this.currentAgentsActionBinding('providers_refresh')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                    )
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                    .accessibilityText($r('app.string.action_refresh'))
                    .onClick(() => {
                      if (!this.isAgentsActionActive(ctrl) || ctrl.hostId.length === 0) {
                        return;
                      }
                      this.store.triggerHapticPreset('selection');
                      this.store.refreshModelsForHostSilent(ctrl.hostId, true);
                    })
                },
                (ctrl: OrbitSettingsControlBinding) => `prov_refresh_${ctrl.renderKey}`
              )
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }

        if (this.providerCatalogs.length === 0) {
          ListItem() {
            Text($r('app.string.settings_agents_no_catalogs'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else {
          ListItemGroup() {
            ForEach(
              this.boundProviderCatalogs,
              (row: AgentsProviderCatalogBinding) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      OrbitProviderBrandIcon({ provider: row.catalog.provider, iconSize: 18 })
                      Column({ space: 2 }) {
                        Text(formatProviderLabel(row.catalog.provider))
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        Text($r('app.string.settings_agents_model_count', row.catalog.models.length))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      if (this.targetDefaultProvider === row.catalog.provider) {
                        SymbolGlyph($r('sys.symbol.checkmark'))
                          .fontSize(15)
                          .fontColor([this.store.palette.accentPrimary])
                      }
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .padding({
                    left: OrbitTokens.SPACE_LG,
                    right: OrbitTokens.SPACE_LG,
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(formatProviderLabel(row.catalog.provider))
                  .onClick(() => {
                    if (!this.isAgentsActionActive(row.control)) {
                      return;
                    }
                    this.store.setHostDefaultProvider(row.control.hostId, row.catalog.provider);
                  })
                }
              },
              (row: AgentsProviderCatalogBinding) => `prov_${row.control.renderKey}_${row.catalog.provider}`
            )
          }
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .clip(true)
          .divider({
            strokeWidth: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline,
            startMargin: OrbitTokens.SPACE_LG,
            endMargin: OrbitTokens.SPACE_LG
          })
        }

        // ACP Registry
        ListItem() {
          Row({ space: OrbitTokens.SPACE_XS }) {
            OrbitSectionHeader({ title: $r('app.string.settings_acp_registry_section') })
            Blank()
            ForEach(
              [this.currentAgentsActionBinding('acp_refresh')],
              (ctrl: OrbitSettingsControlBinding) => {
                Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .enabled(!this.loadingRegistry && ctrl.hostId.length > 0)
                  .accessibilityText($r('app.string.action_refresh'))
                  .onClick(() => {
                    this.refreshRegistry(ctrl);
                  })
              },
              (ctrl: OrbitSettingsControlBinding) => `acp_refresh_${ctrl.renderKey}`
            )
            ForEach(
              [this.currentAgentsActionBinding('acp_toggle_add')],
              (ctrl: OrbitSettingsControlBinding) => {
                Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .accessibilityText($r('app.string.settings_acp_add_custom'))
                  .onClick(() => {
                    if (!this.isAgentsActionActive(ctrl)) {
                      return;
                    }
                    this.showAddCustomAcp = !this.showAddCustomAcp;
                    this.store.triggerHaptic('selection');
                  })
              },
              (ctrl: OrbitSettingsControlBinding) => `acp_add_t_${ctrl.renderKey}`
            )
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }

        if (this.errorText.length > 0) {
          ListItem() {
            Text(this.errorText)
              .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_MD)
          }
        }

        if (this.lastTestResult !== null) {
          ListItem() {
            Text(
              this.lastTestResult.ok
                ? $r(
                  'app.string.settings_acp_test_ok',
                  this.lastTestResult.agent_id,
                  this.lastTestResult.latency_ms ?? 0
                )
                : (this.lastTestResult.error ?? $r('app.string.common_actionFailed'))
            )
              .attributeModifier(
                orbitTextMeta(
                  this.lastTestResult.ok
                    ? this.store.palette.statusDirect
                    : this.store.palette.statusDanger
                )
              )
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_MD)
          }
        }

        if (this.showAddCustomAcp) {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_acp_add_custom'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

              ForEach(
                [this.currentAgentsActionBinding('acp_inputs')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    TextInput({
                      placeholder: $r('app.string.settings_acp_id_placeholder'),
                      text: this.customAcpId
                    })
                      .attributeModifier(
                        orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_acp_id_input_a11y'))
                      .onChange((v: string) => {
                        if (!this.isAgentsActionActive(ctrl)) {
                          return;
                        }
                        this.customAcpId = v;
                      })

                    TextInput({
                      placeholder: $r('app.string.settings_acp_name_placeholder'),
                      text: this.customAcpName
                    })
                      .attributeModifier(
                        orbitInputSubhead(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_acp_name_input_a11y'))
                      .onChange((v: string) => {
                        if (!this.isAgentsActionActive(ctrl)) {
                          return;
                        }
                        this.customAcpName = v;
                      })

                    TextInput({
                      placeholder: $r('app.string.settings_acp_cmd_placeholder'),
                      text: this.customAcpCommand
                    })
                      .attributeModifier(
                        orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_acp_cmd_input_a11y'))
                      .onChange((v: string) => {
                        if (!this.isAgentsActionActive(ctrl)) {
                          return;
                        }
                        this.customAcpCommand = v;
                      })

                    TextInput({
                      placeholder: $r('app.string.settings_acp_args_placeholder'),
                      text: this.customAcpArgs
                    })
                      .attributeModifier(
                        orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_acp_args_input_a11y'))
                      .onChange((v: string) => {
                        if (!this.isAgentsActionActive(ctrl)) {
                          return;
                        }
                        this.customAcpArgs = v;
                      })
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `acp_in_${ctrl.renderKey}`
              )

              ForEach(
                [this.currentAgentsActionBinding('acp_save')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Button($r('app.string.action_save'), { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(
                      orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                    )
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .borderRadius(OrbitTokens.RADIUS_MD)
                    .enabled(
                      !this.savingCustom &&
                      this.customAcpId.trim().length > 0 &&
                      this.customAcpName.trim().length > 0 &&
                      this.customAcpCommand.trim().length > 0
                    )
                    .accessibilityText($r('app.string.action_save'))
                    .onClick(() => {
                      if (!this.isAgentsActionActive(ctrl)) {
                        return;
                      }
                      dismissHarmonyKeyboard(this.getUIContext());
                      this.saveCustomAcpAgent(ctrl);
                    })
                },
                (ctrl: OrbitSettingsControlBinding) => `acp_save_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        }

        if (this.acpAgents.length === 0) {
          ListItem() {
            Text($r('app.string.settings_acp_empty'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else {
          ListItemGroup() {
            ForEach(
              this.boundAcpAgents,
              (row: AgentsAcpRowBinding) => {
                ListItem() {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Column({ space: 3 }) {
                      Row({ space: 6 }) {
                        Text(row.item.spec.name)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .flexShrink(1)
                        Text(
                          row.item.installed
                            ? $r('app.string.settings_acp_installed')
                            : $r('app.string.settings_acp_not_installed')
                        )
                          .attributeModifier(
                            orbitTextMeta(
                              row.item.installed ? this.store.palette.statusDirect : this.store.palette.fgMuted
                            )
                          )
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      Text(`${row.item.spec.command}${row.item.spec.args.length > 0 ? ' ' + row.item.spec.args.join(' ') : ''}`)
                        .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    Button($r('app.string.settings_acp_test_btn'), { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .padding({ left: 10, right: 10 })
                      .enabled(this.testingAgentId.length === 0)
                      .accessibilityText($r('app.string.settings_acp_test_btn'))
                      .onClick(() => {
                        this.testAcpAgent(row.item.spec.id, row.control);
                      })

                    if (row.item.spec.source === 'custom') {
                      Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.subtleSurface)
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: 10, right: 10 })
                        .accessibilityText($r('app.string.action_delete'))
                        .onClick(() => {
                          this.deleteCustomAcpAgent(row.item.spec.id, row.control);
                        })
                    }
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_LG)
                  .alignItems(VerticalAlign.Center)
                }
              },
              (row: AgentsAcpRowBinding) => `acp_${row.control.renderKey}_${row.item.spec.id}`
            )
          }
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .clip(true)
          .divider({
            strokeWidth: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline,
            startMargin: OrbitTokens.SPACE_LG,
            endMargin: OrbitTokens.SPACE_LG
          })
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XS,
        bottom: this.store.keyboardHeightVp > 0
          ? OrbitTokens.SPACE_MD
          : Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
      .onScrollStart(() => {
        dismissHarmonyKeyboard(this.getUIContext());
      })
    }
    .title($r('app.string.settings_item_agents_models'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible || !this.guard.isVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.loadLocalAcpAgents();
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.loadingRegistry = false;
      this.testingAgentId = '';
      this.savingCustom = false;
    })
  }
}

@ComponentV2
export struct OrbitSettingsPermissionsGuardianPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guardianGuard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  private readonly rulesGuard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private guardian: GuardianPolicyConfigDto | null = null;
  @Local private loadingGuardian: boolean = false;
  @Local private savingGuardian: boolean = false;
  @Local private guardianError: string = '';
  @Local private rules: PermissionRuleRecordDto[] = [];
  @Local private loadingRules: boolean = false;
  @Local private rulesError: string = '';
  @Local private showAddRule: boolean = false;
  @Local private ruleToolKind: string = 'shell_command';
  @Local private ruleDecision: string = 'allow';
  @Local private rulePattern: string = '';
  @Local private savingRule: boolean = false;
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private currentPermActionBinding(scopeKey: string): OrbitSettingsControlBinding {
    return createSettingsControlBinding(
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.visibleEpoch,
      scopeKey
    );
  }

  private isPermActionActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      binding.scopeKey
    );
  }

  @Computed
  get boundRuleRows(): PermissionRuleRowBinding[] {
    const control = this.currentPermActionBinding('rules');
    const out: PermissionRuleRowBinding[] = [];
    for (let i = 0; i < this.rules.length; i++) {
      out.push({ rule: this.rules[i], control: control });
    }
    return out;
  }

  @Computed
  get boundGuardianPolicies(): GuardianPolicyBinding[] {
    if (this.guardian === null) {
      return [];
    }
    return [
      {
        policy: this.guardian,
        control: this.currentPermActionBinding('guardian')
      }
    ];
  }

  @Computed
  get boundAccountModes(): PermStringOptionBinding[] {
    const control = this.currentPermActionBinding('perm_account_mode');
    const out: PermStringOptionBinding[] = [];
    for (let i = 0; i < ACCOUNT_MODES.length; i++) {
      out.push({ value: ACCOUNT_MODES[i], control: control });
    }
    return out;
  }

  @Computed
  get boundRuleToolKinds(): PermStringOptionBinding[] {
    const control = this.currentPermActionBinding('perm_rule_kind');
    const out: PermStringOptionBinding[] = [];
    for (let i = 0; i < RULE_TOOL_KINDS.length; i++) {
      out.push({ value: RULE_TOOL_KINDS[i], control: control });
    }
    return out;
  }

  @Computed
  get boundRuleDecisions(): PermStringOptionBinding[] {
    const control = this.currentPermActionBinding('perm_rule_decision');
    const out: PermStringOptionBinding[] = [];
    for (let i = 0; i < RULE_DECISIONS.length; i++) {
      out.push({ value: RULE_DECISIONS[i], control: control });
    }
    return out;
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.guardianGuard.setVisible(true);
    this.rulesGuard.setVisible(true);
    this.loadGuardian();
    this.loadRules();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guardianGuard.unmount();
    this.rulesGuard.unmount();
    this.loadingGuardian = false;
    this.loadingRules = false;
    this.savingGuardian = false;
    this.savingRule = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guardianGuard.invalidate(this.resolvedHostId(), 'guardian');
    this.rulesGuard.invalidate(this.resolvedHostId(), 'rules');
    this.guardian = null;
    this.savingGuardian = false;
    this.guardianError = '';
    this.rules = [];
    this.savingRule = false;
    this.rulesError = '';
    if (this.pageMounted && this.pageVisible) {
      this.loadGuardian();
      this.loadRules();
    }
  }

  private loadGuardian(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loadingGuardian = false;
      return;
    }
    const target = this.resolvedHostId();
    const capturedVisibleEpoch = this.visibleEpoch;
    const token = this.guardianGuard.beginLoad(target, 'guardian', this.store.hostSelectionEpoch, this.rootEpoch);
    if (target.length === 0) {
      this.loadingGuardian = false;
      return;
    }
    this.loadingGuardian = true;
    this.guardianError = '';
    this.store.getGuardianConfigScoped(target).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guardianGuard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loadingGuardian = false;
      if (res.ok && res.data !== null) {
        this.guardian = res.data;
      } else {
        this.guardianError = res.errorDetail;
      }
    });
  }

  private saveGuardian(next: GuardianPolicyConfigDto, binding: OrbitSettingsControlBinding): void {
    if (!this.isPermActionActive(binding) || binding.hostId.length === 0 || this.savingGuardian) {
      return;
    }
    const token = this.guardianGuard.beginItemWrite(
      binding.hostId,
      'guardian',
      'policy',
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.savingGuardian = true;
    this.guardianError = '';
    this.store.updateGuardianConfigScoped(binding.hostId, next).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guardianGuard.isItemWriteCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.savingGuardian = false;
      if (res.ok && res.data !== null) {
        this.guardian = res.data;
        this.store.triggerHapticPreset('success');
      } else {
        this.guardianError = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private cloneGuardianWith(
    field: string,
    boolVal: boolean,
    riskVal: string = ''
  ): GuardianPolicyConfigDto | null {
    const cur = this.guardian;
    if (cur === null) {
      return null;
    }
    return {
      enabled: field === 'enabled' ? boolVal : cur.enabled,
      auto_approve_max_risk: field === 'risk' && riskVal.length > 0 ? riskVal : cur.auto_approve_max_risk,
      use_ai_classifier_for_medium: field === 'ai_medium' ? boolVal : cur.use_ai_classifier_for_medium,
      classifier_timeout_ms: cur.classifier_timeout_ms,
      allow_read_only_tools: field === 'read_only' ? boolVal : cur.allow_read_only_tools,
      allow_workspace_tests: field === 'tests' ? boolVal : cur.allow_workspace_tests,
      blocked_command_patterns: cur.blocked_command_patterns,
      trusted_command_prefixes: cur.trusted_command_prefixes,
      llm_tier2_enabled: field === 'tier2' ? boolVal : cur.llm_tier2_enabled,
      llm_provider: cur.llm_provider,
      llm_model: cur.llm_model,
      llm_timeout_ms: cur.llm_timeout_ms
    };
  }

  private loadRules(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loadingRules = false;
      return;
    }
    const target = this.resolvedHostId();
    const capturedVisibleEpoch = this.visibleEpoch;
    const token = this.rulesGuard.beginLoad(target, 'rules', this.store.hostSelectionEpoch, this.rootEpoch);
    if (target.length === 0) {
      this.loadingRules = false;
      return;
    }
    this.loadingRules = true;
    this.rulesError = '';
    this.store.listPermissionRulesScoped(target, null).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.rulesGuard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loadingRules = false;
      if (res.ok && res.data !== null) {
        this.rules = res.data;
      } else {
        this.rulesError = res.errorDetail;
      }
    });
  }

  private saveNewRule(binding: OrbitSettingsControlBinding): void {
    if (!this.isPermActionActive(binding)) {
      return;
    }
    const target = binding.hostId;
    const pat = this.rulePattern.trim();
    if (target.length === 0 || pat.length === 0 || this.savingRule) {
      return;
    }
    const input: UpsertPermissionRuleInputDto = {
      scope_kind: 'host',
      tool_kind: this.ruleToolKind,
      pattern_kind: 'prefix',
      pattern: pat,
      decision: this.ruleDecision,
      source: 'manual'
    };
    const token = this.rulesGuard.beginItemWrite(
      target,
      'rules',
      'create',
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.savingRule = true;
    this.rulesError = '';
    this.store.upsertPermissionRuleScoped(target, input).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.rulesGuard.isItemWriteCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.savingRule = false;
      if (res.ok && res.data !== null) {
        this.rulePattern = '';
        this.showAddRule = false;
        this.store.triggerHapticPreset('success');
        this.loadRules();
      } else {
        this.rulesError = res.errorDetail;
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private confirmDeleteRule(row: PermissionRuleRowBinding): void {
    if (!this.isPermActionActive(row.control) || row.control.hostId.length === 0) {
      return;
    }
    const rule = row.rule;
    const snap = this.rulesGuard.freezeDeleteConfirm(
      row.control.hostId,
      'rules',
      '',
      rule.id,
      row.control.hostSelectionEpoch,
      row.control.rootEpoch
    );
    this.getUIContext().showAlertDialog({
      title: $r('app.string.settings_rule_delete_title'),
      message: $r('app.string.settings_rule_delete_message', rule.pattern),
      autoCancel: true,
      cancel: () => {
        this.rulesGuard.cancelDeleteConfirm();
      },
      primaryButton: {
        value: $r('app.string.action_cancel'),
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          this.rulesGuard.cancelDeleteConfirm();
        }
      },
      secondaryButton: {
        value: $r('app.string.action_delete'),
        fontColor: this.store.palette.statusDanger,
        action: () => {
          if (
            !this.isPermActionActive(row.control) ||
            !this.rulesGuard.isDeleteConfirmValid(
              snap,
              this.resolvedHostId(),
              'rules',
              '',
              this.store.hostSelectionEpoch,
              this.rootEpoch
            )
          ) {
            return;
          }
          const writeToken = this.rulesGuard.beginItemWrite(
            snap.hostId,
            'rules',
            snap.itemId,
            snap.globalHostEpoch,
            snap.rootEpoch
          );
          this.store.deletePermissionRuleScoped(snap.hostId, snap.itemId).then((res) => {
            if (
              !this.pageMounted ||
              !this.pageVisible ||
              row.control.visibleEpoch !== this.visibleEpoch ||
              !this.rulesGuard.isItemWriteCurrent(writeToken, this.store.hostSelectionEpoch, this.rootEpoch)
            ) {
              return;
            }
            if (res.ok) {
              this.store.triggerHapticPreset('warning');
              this.rules = this.rules.filter((r: PermissionRuleRecordDto): boolean => r.id !== snap.itemId);
            } else {
              this.rulesError = res.errorDetail;
              this.store.triggerHapticPreset('warning');
            }
          });
        }
      }
    });
  }

  private modeLabel(mode: string): Resource {
    if (mode === 'auto') {
      return $r('app.string.settings_mode_auto');
    }
    if (mode === 'accept_edits') {
      return $r('app.string.settings_mode_accept_edits');
    }
    if (mode === 'full_access') {
      return $r('app.string.settings_mode_full_access');
    }
    return $r('app.string.settings_mode_default');
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        // Default account permission mode
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_general_default_mode_section'), isFirst: true })
        }

        ListItem() {
          Row({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.boundAccountModes,
              (row: PermStringOptionBinding) => {
                Button(this.modeLabel(row.value), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(
                      normalizeModeSlug(this.store.composerMode) === row.value
                        ? this.store.palette.onAccent
                        : this.store.palette.fgSecondary,
                      normalizeModeSlug(this.store.composerMode) === row.value
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .layoutWeight(1)
                  .accessibilityText(this.modeLabel(row.value))
                  .onClick(() => {
                    if (!this.isPermActionActive(row.control)) {
                      return;
                    }
                    const res = this.store.setAccountDefaultPermissionMode(row.value);
                    if (!this.isPermActionActive(row.control)) {
                      return;
                    }
                    this.store.triggerHaptic(res.ok ? 'selection' : 'warning');
                  })
              },
              (row: PermStringOptionBinding) => `perm_mode_${row.control.renderKey}_${row.value}`
            )
          }
          .width('100%')
        }

        // Guardian Policy
        ListItem() {
          Row() {
            OrbitSectionHeader({ title: $r('app.string.settings_guardian_section') })
            Blank()
            ForEach(
              [this.currentPermActionBinding('perm_refresh')],
              (ctrl: OrbitSettingsControlBinding) => {
                Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .enabled(!this.loadingGuardian && ctrl.hostId.length > 0)
                  .accessibilityText($r('app.string.action_refresh'))
                  .onClick(() => {
                    if (!this.isPermActionActive(ctrl)) {
                      return;
                    }
                    this.store.triggerHaptic('selection');
                    this.loadGuardian();
                    this.loadRules();
                  })
              },
              (ctrl: OrbitSettingsControlBinding) => `perm_refresh_${ctrl.renderKey}`
            )
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }

        if (this.resolvedHostId().length === 0) {
          ListItem() {
            Text($r('app.string.settings_no_host_selected'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding(OrbitTokens.SPACE_LG)
              .width('100%')
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else {
          if (this.guardianError.length > 0) {
            ListItem() {
              Text(this.guardianError)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          }

          if (this.boundGuardianPolicies.length > 0) {
            ForEach(
              this.boundGuardianPolicies,
              (gRow: GuardianPolicyBinding) => {
                ListItemGroup() {
                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_guardian_enable'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({ type: ToggleType.Switch, isOn: gRow.policy.enabled })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.savingGuardian)
                        .accessibilityText($r('app.string.settings_guardian_enable'))
                        .onChange((val: boolean) => {
                          if (val !== gRow.policy.enabled) {
                            const next = this.cloneGuardianWith('enabled', val);
                            if (next !== null) {
                              this.saveGuardian(next, gRow.control);
                            }
                          }
                        })
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  }

                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_guardian_max_risk'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      ForEach(
                        GUARDIAN_RISK_LEVELS,
                        (lvl: string) => {
                          Button(
                            lvl === 'low'
                              ? $r('app.string.settings_risk_low')
                              : $r('app.string.settings_risk_medium'),
                            { type: ButtonType.Capsule, stateEffect: true }
                          )
                            .attributeModifier(
                              orbitButtonMeta(
                                gRow.policy.auto_approve_max_risk === lvl
                                  ? this.store.palette.onAccent
                                  : this.store.palette.fgSecondary,
                                gRow.policy.auto_approve_max_risk === lvl
                                  ? this.store.palette.accentPrimaryFill
                                  : this.store.palette.subtleSurface
                              )
                            )
                            .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                            .padding({ left: 10, right: 10 })
                            .enabled(!this.savingGuardian)
                            .accessibilityText(
                              lvl === 'low'
                                ? $r('app.string.settings_risk_low')
                                : $r('app.string.settings_risk_medium')
                            )
                            .onClick(() => {
                              const next = this.cloneGuardianWith('risk', false, lvl);
                              if (next !== null) {
                                this.saveGuardian(next, gRow.control);
                              }
                            })
                        },
                        (lvl: string) => `grisk_${gRow.control.renderKey}_${lvl}`
                      )
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                    .alignItems(VerticalAlign.Center)
                  }

                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_guardian_read_only'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({ type: ToggleType.Switch, isOn: gRow.policy.allow_read_only_tools })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.savingGuardian)
                        .accessibilityText($r('app.string.settings_guardian_read_only'))
                        .onChange((val: boolean) => {
                          if (val !== gRow.policy.allow_read_only_tools) {
                            const next = this.cloneGuardianWith('read_only', val);
                            if (next !== null) {
                              this.saveGuardian(next, gRow.control);
                            }
                          }
                        })
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  }

                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_guardian_workspace_tests'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({ type: ToggleType.Switch, isOn: gRow.policy.allow_workspace_tests })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.savingGuardian)
                        .accessibilityText($r('app.string.settings_guardian_workspace_tests'))
                        .onChange((val: boolean) => {
                          if (val !== gRow.policy.allow_workspace_tests) {
                            const next = this.cloneGuardianWith('tests', val);
                            if (next !== null) {
                              this.saveGuardian(next, gRow.control);
                            }
                          }
                        })
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  }

                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_guardian_tier2'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({ type: ToggleType.Switch, isOn: gRow.policy.llm_tier2_enabled })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.savingGuardian)
                        .accessibilityText($r('app.string.settings_guardian_tier2'))
                        .onChange((val: boolean) => {
                          if (val !== gRow.policy.llm_tier2_enabled) {
                            const next = this.cloneGuardianWith('tier2', val);
                            if (next !== null) {
                              this.saveGuardian(next, gRow.control);
                            }
                          }
                        })
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                  }
                }
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
                .clip(true)
                .divider({
                  strokeWidth: OrbitTokens.HAIRLINE,
                  color: this.store.palette.hairline,
                  startMargin: OrbitTokens.SPACE_LG,
                  endMargin: OrbitTokens.SPACE_LG
                })
              },
              (gRow: GuardianPolicyBinding) =>
                `guard_${gRow.control.renderKey}_${gRow.policy.enabled ? '1' : '0'}_${gRow.policy.auto_approve_max_risk}`
            )
          }

          // Permission Rules
          ListItem() {
            Row() {
              OrbitSectionHeader({ title: $r('app.string.settings_rules_section') })
              Blank()
              ForEach(
                [this.currentPermActionBinding('rules_toggle_add')],
                (ctrl: OrbitSettingsControlBinding) => {
                  Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                    )
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                    .accessibilityText($r('app.string.settings_rule_add_title'))
                    .onClick(() => {
                      if (!this.isPermActionActive(ctrl)) {
                        return;
                      }
                      this.showAddRule = !this.showAddRule;
                      this.store.triggerHaptic('selection');
                    })
                },
                (ctrl: OrbitSettingsControlBinding) => `rules_add_t_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }

          if (this.rulesError.length > 0) {
            ListItem() {
              Text(this.rulesError)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          }

          if (this.showAddRule) {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Text($r('app.string.settings_rule_add_title'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

                Scroll() {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    ForEach(
                      this.boundRuleToolKinds,
                      (row: PermStringOptionBinding) => {
                        Button(row.value, { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(
                              this.ruleToolKind === row.value
                                ? this.store.palette.onAccent
                                : this.store.palette.fgSecondary,
                              this.ruleToolKind === row.value
                                ? this.store.palette.accentPrimaryFill
                                : this.store.palette.subtleSurface
                            )
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: 10, right: 10 })
                          .accessibilityText($r('app.string.settings_rule_tool_kind_a11y', row.value))
                          .onClick(() => {
                            if (!this.isPermActionActive(row.control)) {
                              return;
                            }
                            this.ruleToolKind = row.value;
                          })
                      },
                      (row: PermStringOptionBinding) => `rtk_${row.control.renderKey}_${row.value}`
                    )
                  }
                }
                .scrollable(ScrollDirection.Horizontal)
                .scrollBar(BarState.Off)
                .width('100%')

                Row({ space: OrbitTokens.SPACE_XS }) {
                  ForEach(
                    this.boundRuleDecisions,
                    (row: PermStringOptionBinding) => {
                      Button(row.value, { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.ruleDecision === row.value
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.ruleDecision === row.value
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_rule_decision_a11y', row.value))
                        .onClick(() => {
                          if (!this.isPermActionActive(row.control)) {
                            return;
                          }
                          this.ruleDecision = row.value;
                        })
                    },
                    (row: PermStringOptionBinding) => `rdec_${row.control.renderKey}_${row.value}`
                  )
                }
                .width('100%')

                ForEach(
                  [this.currentPermActionBinding('rules_pattern')],
                  (ctrl: OrbitSettingsControlBinding) => {
                    TextInput({
                      placeholder: $r('app.string.settings_rule_pattern_placeholder'),
                      text: this.rulePattern
                    })
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          this.store.palette.subtleSurface
                        )
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_rule_pattern_input_a11y'))
                      .onChange((v: string) => {
                        if (!this.isPermActionActive(ctrl)) {
                          return;
                        }
                        this.rulePattern = v;
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `rules_pat_${ctrl.renderKey}`
                )

                ForEach(
                  [this.currentPermActionBinding('rules_save')],
                  (ctrl: OrbitSettingsControlBinding) => {
                    Button($r('app.string.action_save'), { type: ButtonType.Normal, stateEffect: true })
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .enabled(!this.savingRule && this.rulePattern.trim().length > 0)
                      .accessibilityText($r('app.string.action_save'))
                      .onClick(() => {
                        if (!this.isPermActionActive(ctrl)) {
                          return;
                        }
                        dismissHarmonyKeyboard(this.getUIContext());
                        this.saveNewRule(ctrl);
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `rules_save_${ctrl.renderKey}`
                )
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          }

          if (this.rules.length === 0 && !this.loadingRules) {
            ListItem() {
              Text($r('app.string.settings_rules_empty'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_LG)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
            }
          } else if (this.boundRuleRows.length > 0) {
            ListItemGroup() {
              ForEach(
                this.boundRuleRows,
                (row: PermissionRuleRowBinding) => {
                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Column({ space: 3 }) {
                        Text(row.rule.pattern)
                          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(2)
                        Text(`${row.rule.decision} · ${row.rule.tool_kind} · ${row.rule.scope_kind}`)
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.subtleSurface)
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: 10, right: 10 })
                        .accessibilityText($r('app.string.action_delete'))
                        .onClick(() => {
                          this.confirmDeleteRule(row);
                        })
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                    .alignItems(VerticalAlign.Center)
                  }
                },
                (row: PermissionRuleRowBinding) => `prule_${row.control.renderKey}_${row.rule.id}`
              )
            }
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .clip(true)
            .divider({
              strokeWidth: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline,
              startMargin: OrbitTokens.SPACE_LG,
              endMargin: OrbitTokens.SPACE_LG
            })
          }
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XS,
        bottom: this.store.keyboardHeightVp > 0
          ? OrbitTokens.SPACE_MD
          : Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
      .onScrollStart(() => {
        dismissHarmonyKeyboard(this.getUIContext());
      })
    }
    .title($r('app.string.settings_item_permissions_guardian'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible || !this.guardianGuard.isVisible || !this.rulesGuard.isVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guardianGuard.setVisible(true);
        this.rulesGuard.setVisible(true);
        this.loadGuardian();
        this.loadRules();
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guardianGuard.setVisible(false);
      this.rulesGuard.setVisible(false);
      this.loadingGuardian = false;
      this.loadingRules = false;
      this.savingGuardian = false;
      this.savingRule = false;
    })
  }
}

@ComponentV2
export struct OrbitSettingsUsageQuotasPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private selectedDays: number = 7;
  @Local private snapshot: UsageInsightsSnapshotDto | null = null;
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private visibleEpoch: number = 1;
  @Local private periodItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: $r('app.string.settings_usage_7d') },
    { text: $r('app.string.settings_usage_30d') }
  ]);

  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  @Computed
  get periodIndex(): number {
    return this.selectedDays === 30 ? 1 : 0;
  }

  @Computed
  get quotas(): ProviderQuotaDto[] {
    const target = this.resolvedHostId();
    const detail = target.length > 0 ? this.store.hostDetailFor(target) : null;
    return detail !== null ? detail.quotas : [];
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadUsage();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.loading = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), `usage_${this.selectedDays}`);
    this.snapshot = null;
    this.errorText = '';
    this.loading = false;
    if (this.pageMounted && this.pageVisible) {
      this.loadUsage();
    }
  }

  private loadUsage(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const target = this.resolvedHostId();
    const days = this.selectedDays;
    const capturedVisibleEpoch = this.visibleEpoch;
    const token = this.guard.beginLoad(
      target,
      `usage_${days}`,
      this.store.hostSelectionEpoch,
      this.rootEpoch
    );
    if (target.length === 0) {
      this.loading = false;
      this.snapshot = null;
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.refreshQuotasForHostSilent(target);
    this.store.getUsageInsightsScoped(target, days).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loading = false;
      if (res.ok && res.data !== null) {
        this.snapshot = res.data;
        this.errorText = '';
      } else {
        this.errorText = res.errorDetail;
      }
    });
  }

  private quotaSemanticColor(usedPercent: number): ResourceColor {
    if (usedPercent > 90) {
      return this.store.palette.statusDanger;
    }
    if (usedPercent >= 70) {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.statusDirect;
  }

  private formatTokenCompact(n: number): string {
    if (n >= 1000000) {
      return `${(n / 1000000).toFixed(2)}M`;
    }
    if (n >= 1000) {
      return `${(n / 1000).toFixed(1)}k`;
    }
    return `${n}`;
  }

  private usageControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isUsageControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      binding.scopeKey
    );
  }

  private selectUsagePeriod(binding: OrbitSettingsControlBinding, idx: number): void {
    if (!this.isUsageControlActive(binding)) {
      return;
    }
    if (idx >= 0 && idx < ORBIT_USAGE_PERIOD_DAYS.length) {
      const nextDays = ORBIT_USAGE_PERIOD_DAYS[idx];
      if (nextDays !== this.selectedDays) {
        this.selectedDays = nextDays;
        this.snapshot = null;
        this.errorText = '';
        this.store.triggerHapticPreset('selection');
        this.loadUsage();
      }
    }
  }

  private refreshUsage(binding: OrbitSettingsControlBinding): void {
    if (!this.isUsageControlActive(binding)) {
      return;
    }
    this.store.triggerHaptic('selection');
    this.loadUsage();
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        // Period switcher + Refresh
        ListItem() {
          Row({ space: OrbitTokens.SPACE_SM }) {
            ForEach(
              this.usageControlBindings('usage_period'),
              (binding: OrbitSettingsControlBinding) => {
                CapsuleSegmentButtonV2({
                  items: this.periodItems,
                  selectedIndex: this.periodIndex,
                  onItemClicked: (idx: number) => {
                    this.selectUsagePeriod(binding, idx);
                  },
                  itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
                  itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
                  itemMaxFontScale: OrbitTokens.MAX_FONT_SCALE_CHROME,
                  itemMinHeight: LengthMetrics.vp(36)
                })
                  .layoutWeight(1)
              },
              (binding: OrbitSettingsControlBinding) => `usage_period_${binding.renderKey}`
            )

            ForEach(
              this.usageControlBindings('usage_refresh'),
              (binding: OrbitSettingsControlBinding) => {
                Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .enabled(!this.loading)
                  .accessibilityText($r('app.string.action_refresh'))
                  .onClick(() => {
                    this.refreshUsage(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) => `usage_refresh_${binding.renderKey}`
            )
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }

        if (this.resolvedHostId().length === 0) {
          ListItem() {
            Text($r('app.string.settings_no_host_selected'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding(OrbitTokens.SPACE_LG)
              .width('100%')
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else {
          if (this.loading && this.snapshot === null) {
            ListItem() {
              Row({ space: OrbitTokens.SPACE_SM }) {
                LoadingProgress().width(18).height(18).color(this.store.palette.accentPrimary)
                Text($r('app.string.common_loading'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
            }
          }

          if (this.errorText.length > 0) {
            ListItem() {
              Text(this.errorText)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          }

          if (this.snapshot !== null) {
            ListItem() {
              OrbitSectionHeader({ title: $r('app.string.settings_usage_summary_section'), isFirst: true })
            }

            ListItemGroup() {
              ListItem() {
                Row() {
                  Text($r('app.string.settings_usage_total_cost'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  Blank()
                  Text(`$${this.snapshot.total_cost_usd.toFixed(2)}`)
                    .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              }

              ListItem() {
                Row() {
                  Text($r('app.string.settings_usage_cache_savings'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  Blank()
                  Text(`$${this.snapshot.total_cache_savings_usd.toFixed(2)}`)
                    .attributeModifier(orbitTextMono(this.store.palette.statusDirect))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              }

              ListItem() {
                Row() {
                  Text($r('app.string.settings_usage_tokens_turns'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  Blank()
                  Text(
                    `${this.formatTokenCompact(this.snapshot.total_input_tokens + this.snapshot.total_output_tokens)} · ${this.snapshot.total_turns}`
                  )
                    .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              }
            }
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .clip(true)
            .divider({
              strokeWidth: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline,
              startMargin: OrbitTokens.SPACE_LG,
              endMargin: OrbitTokens.SPACE_LG
            })

            if (this.snapshot.by_model.length > 0) {
              ListItem() {
                OrbitSectionHeader({ title: $r('app.string.settings_usage_by_model') })
              }
              ListItemGroup() {
                ForEach(
                  this.snapshot.by_model.slice(0, 6),
                  (row: UsageModelBreakdownDto) => {
                    ListItem() {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Column({ space: 2 }) {
                          Text(row.model)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                          Text(`${formatProviderLabel(row.provider)} · ${this.formatTokenCompact(row.input_tokens + row.output_tokens)}`)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        }
                        .alignItems(HorizontalAlign.Start)
                        .layoutWeight(1)

                        Text(`$${row.cost_usd.toFixed(2)}`)
                          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 8, bottom: 8 })
                    }
                  },
                  (row: UsageModelBreakdownDto) => `um_${row.provider}_${row.model}`
                )
              }
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .clip(true)
              .divider({
                strokeWidth: OrbitTokens.HAIRLINE,
                color: this.store.palette.hairline,
                startMargin: OrbitTokens.SPACE_LG,
                endMargin: OrbitTokens.SPACE_LG
              })
            }

            if (this.snapshot.by_project.length > 0) {
              ListItem() {
                OrbitSectionHeader({ title: $r('app.string.settings_usage_by_project') })
              }
              ListItemGroup() {
                ForEach(
                  this.snapshot.by_project.slice(0, 5),
                  (proj: UsageProjectBreakdownDto) => {
                    ListItem() {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Column({ space: 2 }) {
                          Text(basenameOfPath(proj.workspace_cwd) || proj.workspace_cwd)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                          Text(`${this.formatTokenCompact(proj.total_tokens)} · ${proj.turns}`)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        }
                        .alignItems(HorizontalAlign.Start)
                        .layoutWeight(1)

                        Text(`$${proj.cost_usd.toFixed(2)}`)
                          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 8, bottom: 8 })
                    }
                  },
                  (proj: UsageProjectBreakdownDto) => `up_${proj.workspace_cwd}`
                )
              }
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .clip(true)
              .divider({
                strokeWidth: OrbitTokens.HAIRLINE,
                color: this.store.palette.hairline,
                startMargin: OrbitTokens.SPACE_LG,
                endMargin: OrbitTokens.SPACE_LG
              })
            }

            if (this.snapshot.forecasts.length > 0) {
              ListItem() {
                OrbitSectionHeader({ title: $r('app.string.settings_usage_forecasts') })
              }
              ListItemGroup() {
                ForEach(
                  this.snapshot.forecasts,
                  (fc: QuotaDepletionForecastDto) => {
                    ListItem() {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Column({ space: 2 }) {
                          Text(`${formatProviderLabel(fc.provider)} · ${fc.window_label}`)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          Text(`${fc.burn_rate_pct_per_hour.toFixed(1)}%/h`)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        }
                        .alignItems(HorizontalAlign.Start)
                        .layoutWeight(1)

                        Text(`${Math.round(fc.remaining_percent)}%`)
                          .attributeModifier(
                            orbitTextMono(
                              fc.likely_to_exhaust_before_reset
                                ? this.store.palette.statusWarning
                                : this.store.palette.statusDirect
                            )
                          )
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 8, bottom: 8 })
                    }
                  },
                  (fc: QuotaDepletionForecastDto) => `fc_${fc.provider}_${fc.window_label}`
                )
              }
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .clip(true)
              .divider({
                strokeWidth: OrbitTokens.HAIRLINE,
                color: this.store.palette.hairline,
                startMargin: OrbitTokens.SPACE_LG,
                endMargin: OrbitTokens.SPACE_LG
              })
            }

            if (this.snapshot.daily.length > 0) {
              ListItem() {
                OrbitSectionHeader({ title: $r('app.string.settings_usage_daily') })
              }
              ListItemGroup() {
                ForEach(
                  this.snapshot.daily.slice(-7),
                  (pt: UsageDailyPointDto) => {
                    ListItem() {
                      Row() {
                        Text(pt.date)
                          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        Blank()
                        Text(`${this.formatTokenCompact(pt.input_tokens + pt.output_tokens)} · $${pt.cost_usd.toFixed(2)}`)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .width('100%')
                      .constraintSize({ minHeight: 40 })
                      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                    }
                  },
                  (pt: UsageDailyPointDto) => `ud_${pt.date}`
                )
              }
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .clip(true)
              .divider({
                strokeWidth: OrbitTokens.HAIRLINE,
                color: this.store.palette.hairline,
                startMargin: OrbitTokens.SPACE_LG,
                endMargin: OrbitTokens.SPACE_LG
              })
            }
          }

          // Live Provider Quotas
          ListItem() {
            OrbitSectionHeader({ title: $r('app.string.settings_usage_live_quotas') })
          }

          ListItemGroup() {
            if (this.quotas.length === 0) {
              ListItem() {
                Row() {
                  Text($r('app.string.settings_usage_no_quotas'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              }
            } else {
              ForEach(
                this.quotas,
                (q: ProviderQuotaDto) => {
                  ListItem() {
                    Column({ space: OrbitTokens.SPACE_SM }) {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        OrbitProviderBrandIcon({ provider: q.provider, iconSize: 16 })
                        Text(formatProviderLabel(q.provider))
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        if (q.account_label !== null && q.account_label.length > 0) {
                          Text(q.account_label)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .flexShrink(1)
                        }
                        Blank()
                        if (q.credits_remaining !== null) {
                          Text(`$${q.credits_remaining.toFixed(2)}`)
                            .attributeModifier(orbitTextMono(this.store.palette.statusDirect, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        }
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)

                      if (q.error !== null && q.error.length > 0) {
                        Text(q.error)
                          .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .width('100%')
                      }

                      if (q.windows.length > 0) {
                        ForEach(
                          q.windows,
                          (win: QuotaWindowDto) => {
                            Column({ space: 5 }) {
                              Row({ space: OrbitTokens.SPACE_SM }) {
                                Text(win.label)
                                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                                  .layoutWeight(1)
                                  .maxLines(1)
                                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                                if (formatQuotaResetCountdown(win.resets_at).length > 0) {
                                  Text(formatQuotaResetCountdown(win.resets_at))
                                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                                    .maxLines(1)
                                }
                                Text(`${Math.round(win.used_percent)}%`)
                                  .attributeModifier(
                                    orbitTextMono(this.quotaSemanticColor(win.used_percent), FontWeight.Medium)
                                  )
                                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                              }
                              .width('100%')
                              .alignItems(VerticalAlign.Center)

                              Progress({
                                value: Math.min(100, Math.max(0, Math.round(win.used_percent))),
                                total: 100,
                                type: ProgressType.Linear
                              })
                                .width('100%')
                                .color(this.quotaSemanticColor(win.used_percent))
                                .backgroundColor(this.store.palette.subtleSurface)
                                .style({ strokeWidth: 4, enableSmoothEffect: !this.store.shellStore.reducedMotion })
                            }
                            .width('100%')
                          },
                          (win: QuotaWindowDto) => `qw_${q.provider}_${win.label}`
                        )
                      }
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                  }
                },
                (q: ProviderQuotaDto) => `pq_${q.provider}`
              )
            }
          }
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .clip(true)
          .divider({
            strokeWidth: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline,
            startMargin: OrbitTokens.SPACE_LG,
            endMargin: OrbitTokens.SPACE_LG
          })
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XS,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_usage_quotas'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.loadUsage();
      }
    })
    .onHidden(() => {
      this.hasHiddenSinceAppear = true;
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.loading = false;
    })
  }
}
