import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  ExternalMcpServerRecordDto,
  ManagedSkillRecordDto,
  SandboxPolicyDto,
  basenameOfPath,
  nonBlankPath
} from '../../model/OrbitTypes';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitSectionHeader } from '../common/OrbitCommonControls';
import {
  OrbitMcpRowBinding,
  OrbitMemoryCwdOptionBinding,
  OrbitMemoryRowBinding,
  OrbitPluginRowBinding,
  OrbitSettingsControlBinding,
  OrbitSettingsRequestGuard,
  applySettledMcpToggle,
  applySettledMemoryConfirm,
  applySettledMemoryDelete,
  applySettledPluginToggle,
  canSettleMcpRowWrite,
  canSettleMemoryRowWrite,
  canSettlePluginRowWrite,
  createMemoryCwdOptionBinding,
  createSettingsControlBinding,
  isMcpRowBindingActive,
  isMemoryCwdOptionBindingActive,
  isMemoryRowBindingActive,
  isPluginRowBindingActive,
  isSettingsControlBindingActive,
  resolveEffectiveMemoryCwd,
  resolveHostScopedExtensionCwdPure
} from './OrbitSettingsNavModel';

/**
 * Collects exact (untrimmed) registered/discovered project directories belonging exclusively
 * to `targetHost`. NEVER falls back to another host's `activeCwd`.
 */
function collectHostProjectCwds(store: OrbitHarmonyStore, targetHost: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const addExact = (raw: string): void => {
    const exact = nonBlankPath(raw);
    if (exact.length > 0 && exact !== '~' && !seen.has(exact)) {
      seen.add(exact);
      out.push(exact);
    }
  };
  if (targetHost.length === 0) {
    return out;
  }
  addExact(store.getActiveCwdForHost(targetHost));
  for (let i = 0; i < store.projects.length; i++) {
    const proj = store.projects[i];
    if (proj.host_id === targetHost) {
      for (let j = 0; j < proj.directories.length; j++) {
        addExact(proj.directories[j]);
      }
    }
  }
  const detail = store.hostDetailFor(targetHost);
  if (detail !== null) {
    for (let i = 0; i < detail.workspaces.length; i++) {
      addExact(detail.workspaces[i].cwd);
    }
    for (let i = 0; i < detail.discovered_projects.length; i++) {
      addExact(detail.discovered_projects[i].path);
    }
    for (let i = 0; i < detail.agents.length; i++) {
      addExact(detail.agents[i].cwd);
    }
  }
  return out;
}

/**
 * Resolves the exact (untrimmed) project `cwd` strictly scoped to `targetHost` for MCP & Skills.
 * Reads ONLY `targetHost`'s own active directory (`store.getActiveCwdForHost(targetHost)`) and
 * `targetHost`'s registered/discovered directories — NEVER inherits another host's `activeCwd`,
 * degrading to `null` (host-only query) when `targetHost` has no directories.
 */
function resolveHostScopedExtensionCwd(
  store: OrbitHarmonyStore,
  targetHost: string,
  includeProjectScope: boolean,
  explicitSelection: string
): string | null {
  return resolveHostScopedExtensionCwdPure(
    targetHost,
    targetHost,
    store.getActiveCwdForHost(targetHost),
    collectHostProjectCwds(store, targetHost),
    includeProjectScope,
    explicitSelection
  );
}

@ComponentV2
export struct OrbitSettingsMcpPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private serverBindings: OrbitMcpRowBinding[] = [];
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private includeProjectScope: boolean = true;
  @Local private selectedProjectCwd: string = '';
  @Local private togglingIds: string[] = [];
  @Local private loadedListSeq: number = 0;
  @Local private loadedHostId: string = '';
  @Local private loadedScopeKey: string = '';
  @Local private visibleEpoch: number = 1;
  private nextLoadSeq: number = 0;
  private mutationEpoch: number = 0;
  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private resolvedProjectCwd(): string | null {
    return resolveHostScopedExtensionCwd(
      this.store,
      this.resolvedHostId(),
      this.includeProjectScope,
      this.selectedProjectCwd
    );
  }

  private currentScopeKey(): string {
    return this.resolvedProjectCwd() ?? '__host__';
  }

  private clearLoadedServersState(): void {
    this.serverBindings = [];
    this.loadedListSeq = 0;
    this.loadedHostId = '';
    this.loadedScopeKey = '';
    this.togglingIds = [];
  }

  private isMcpBindingActionable(binding: OrbitMcpRowBinding): boolean {
    return isMcpRowBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.currentScopeKey(),
      this.serverBindings
    );
  }

  private canSettleMcpBinding(binding: OrbitMcpRowBinding): boolean {
    return canSettleMcpRowWrite(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.loadedHostId,
      this.loadedScopeKey,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.currentScopeKey()
    );
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadServers();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.clearLoadedServersState();
    this.loading = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), '__reset__');
    this.clearLoadedServersState();
    this.selectedProjectCwd = '';
    this.errorText = '';
    this.loading = false;
    if (this.pageMounted && this.pageVisible) {
      this.loadServers();
    }
  }

  private loadServers(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const targetHost = this.resolvedHostId();
    const targetCwd = this.resolvedProjectCwd();
    const scopeKey = targetCwd ?? '__host__';
    const capturedGlobalHostEpoch = this.store.hostSelectionEpoch;
    const capturedRootEpoch = this.rootEpoch;
    const capturedVisibleEpoch = this.visibleEpoch;
    const scopeChanged =
      this.loadedListSeq === 0 ||
      this.loadedHostId !== targetHost ||
      this.loadedScopeKey !== scopeKey;
    if (scopeChanged) {
      this.clearLoadedServersState();
    }
    const capturedMutationEpoch = this.mutationEpoch;
    const token = this.guard.beginLoad(
      targetHost,
      scopeKey,
      capturedGlobalHostEpoch,
      capturedRootEpoch
    );
    if (targetHost.length === 0) {
      this.loading = false;
      this.clearLoadedServersState();
      this.errorText = '';
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.listExternalMcpServersScoped(targetHost, targetCwd).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loading = false;
      if (this.mutationEpoch !== capturedMutationEpoch) {
        return;
      }
      if (res.ok && res.data !== null) {
        this.nextLoadSeq += 1;
        const batchSeq = this.nextLoadSeq;
        const nextBindings: OrbitMcpRowBinding[] = [];
        for (let i = 0; i < res.data.length; i++) {
          nextBindings.push({
            loadSeq: batchSeq,
            hostId: targetHost,
            hostSelectionEpoch: capturedGlobalHostEpoch,
            rootEpoch: capturedRootEpoch,
            visibleEpoch: capturedVisibleEpoch,
            scopeKey: scopeKey,
            requestCwd: targetCwd,
            server: res.data[i]
          });
        }
        this.loadedListSeq = batchSeq;
        this.loadedHostId = targetHost;
        this.loadedScopeKey = scopeKey;
        this.serverBindings = nextBindings;
        this.errorText = '';
      } else {
        if (this.loadedHostId !== targetHost || this.loadedScopeKey !== scopeKey) {
          this.clearLoadedServersState();
        }
        this.errorText = res.errorDetail;
      }
    });
  }

  private isToggling(id: string): boolean {
    return this.togglingIds.includes(id);
  }

  private setToggling(id: string, busy: boolean): void {
    const next: string[] = [];
    for (let i = 0; i < this.togglingIds.length; i++) {
      if (this.togglingIds[i] !== id) {
        next.push(this.togglingIds[i]);
      }
    }
    if (busy) {
      next.push(id);
    }
    this.togglingIds = next;
  }

  private toggleServer(binding: OrbitMcpRowBinding, nextEnabled: boolean): void {
    if (this.isToggling(binding.server.id) || !this.isMcpBindingActionable(binding)) {
      return;
    }
    this.mutationEpoch += 1;
    const writeToken = this.guard.beginItemWrite(
      binding.hostId,
      binding.scopeKey,
      binding.server.id,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.setToggling(binding.server.id, true);
    this.errorText = '';
    this.store
      .toggleExternalMcpServerScoped(binding.hostId, binding.server.id, nextEnabled, binding.requestCwd)
      .then((res) => {
        if (!this.guard.isItemWriteCurrent(writeToken, this.store.hostSelectionEpoch, this.rootEpoch)) {
          return;
        }
        if (!this.canSettleMcpBinding(binding)) {
          return;
        }
        this.setToggling(binding.server.id, false);
        if (res.ok) {
          this.mutationEpoch += 1;
          this.store.triggerHapticPreset('selection');
          this.serverBindings = applySettledMcpToggle(this.serverBindings, binding.server.id, nextEnabled);
        } else {
          this.store.triggerHapticPreset('warning');
          this.errorText = res.errorDetail;
          this.loadServers();
        }
      });
  }

  private transportSummary(server: ExternalMcpServerRecordDto): string {
    if (server.transport.kind === 'stdio') {
      const cmd = server.transport.command ?? '';
      const args = server.transport.args !== undefined ? server.transport.args.join(' ') : '';
      return args.length > 0 ? `${cmd} ${args}` : cmd;
    }
    return server.transport.url ?? server.transport.kind;
  }

  private mcpControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
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

  private isMcpControlActive(binding: OrbitSettingsControlBinding): boolean {
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

  private toggleIncludeProjectScope(binding: OrbitSettingsControlBinding): void {
    if (!this.isMcpControlActive(binding)) {
      return;
    }
    this.includeProjectScope = !this.includeProjectScope;
    this.clearLoadedServersState();
    this.errorText = '';
    this.store.triggerHapticPreset('selection');
    this.loadServers();
  }

  private refreshServers(binding: OrbitSettingsControlBinding): void {
    if (!this.isMcpControlActive(binding)) {
      return;
    }
    this.store.triggerHapticPreset('selection');
    this.loadServers();
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Row({ space: OrbitTokens.SPACE_SM }) {
            ForEach(
              this.mcpControlBindings('mcp_scope'),
              (binding: OrbitSettingsControlBinding) => {
                Button(
                  this.includeProjectScope
                    ? $r('app.string.settings_ext_scope_project')
                    : $r('app.string.settings_ext_scope_host'),
                  { type: ButtonType.Capsule, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .accessibilityText($r('app.string.settings_ext_scope_toggle_a11y'))
                  .onClick(() => {
                    this.toggleIncludeProjectScope(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) =>
                `mcp_scope_${binding.renderKey}_${this.includeProjectScope ? '1' : '0'}`
            )

            Blank()

            ForEach(
              this.mcpControlBindings('mcp_refresh'),
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
                    this.refreshServers(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) => `mcp_refresh_${binding.renderKey}`
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
        } else if (this.loading && this.serverBindings.length === 0) {
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
        } else {
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

          if (this.serverBindings.length === 0) {
            ListItem() {
              Text($r('app.string.settings_mcp_empty'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_LG)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
            }
          } else {
            ListItemGroup() {
              ForEach(
                this.serverBindings,
                (binding: OrbitMcpRowBinding) => {
                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Column({ space: 4 }) {
                        Row({ space: 6 }) {
                          Text(binding.server.name)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .flexShrink(1)
                          Text(binding.server.scope)
                            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                            .backgroundColor(this.store.palette.subtleSurface)
                            .borderRadius(OrbitTokens.RADIUS_SM)
                        }
                        .alignItems(VerticalAlign.Center)

                        Text(this.transportSummary(binding.server))
                          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(2)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })

                        if (binding.server.last_probe !== undefined && binding.server.last_probe !== null) {
                          Text(
                            binding.server.last_probe.ok
                              ? $r(
                                'app.string.settings_mcp_probe_ok',
                                binding.server.last_probe.tools.length,
                                binding.server.last_probe.latency_ms
                              )
                              : (binding.server.last_probe.error ?? $r('app.string.common_actionFailed'))
                          )
                            .attributeModifier(
                              orbitTextMeta(
                                binding.server.last_probe.ok
                                  ? this.store.palette.statusDirect
                                  : this.store.palette.statusWarning
                              )
                            )
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(2)
                        }
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      Toggle({ type: ToggleType.Switch, isOn: binding.server.enabled })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.isToggling(binding.server.id) && this.isMcpBindingActionable(binding))
                        .accessibilityText(binding.server.name)
                        .onChange((val: boolean) => {
                          if (val !== binding.server.enabled) {
                            this.toggleServer(binding, val);
                          }
                        })
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                    .alignItems(VerticalAlign.Center)
                  }
                },
                (binding: OrbitMcpRowBinding) =>
                  `${binding.loadSeq}::${binding.visibleEpoch}::${binding.server.id}::${binding.server.enabled ? '1' : '0'}`
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
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_mcp'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.togglingIds = [];
        this.loadServers();
      }
    })
    .onHidden(() => {
      this.hasHiddenSinceAppear = true;
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.clearLoadedServersState();
      this.loading = false;
    })
  }
}

@ComponentV2
export struct OrbitSettingsSkillsPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private skills: ManagedSkillRecordDto[] = [];
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private includeProjectScope: boolean = true;
  @Local private selectedProjectCwd: string = '';
  @Local private visibleEpoch: number = 1;
  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private resolvedProjectCwd(): string | null {
    return resolveHostScopedExtensionCwd(
      this.store,
      this.resolvedHostId(),
      this.includeProjectScope,
      this.selectedProjectCwd
    );
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadSkills();
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
    this.guard.invalidate(this.resolvedHostId(), '__reset__');
    this.skills = [];
    this.selectedProjectCwd = '';
    this.errorText = '';
    this.loading = false;
    if (this.pageMounted && this.pageVisible) {
      this.loadSkills();
    }
  }

  private loadSkills(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const targetHost = this.resolvedHostId();
    const targetCwd = this.resolvedProjectCwd();
    const capturedVisibleEpoch = this.visibleEpoch;
    const token = this.guard.beginLoad(
      targetHost,
      targetCwd ?? '__host__',
      this.store.hostSelectionEpoch,
      this.rootEpoch
    );
    if (targetHost.length === 0) {
      this.loading = false;
      this.skills = [];
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.listManagedSkillsScoped(targetHost, targetCwd).then((res) => {
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
        this.skills = res.data;
        this.errorText = '';
      } else {
        this.errorText = res.errorDetail;
      }
    });
  }

  private skillsControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
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

  private isSkillsControlActive(binding: OrbitSettingsControlBinding): boolean {
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

  private toggleSkillsProjectScope(binding: OrbitSettingsControlBinding): void {
    if (!this.isSkillsControlActive(binding)) {
      return;
    }
    this.includeProjectScope = !this.includeProjectScope;
    this.skills = [];
    this.errorText = '';
    this.store.triggerHapticPreset('selection');
    this.loadSkills();
  }

  private refreshSkills(binding: OrbitSettingsControlBinding): void {
    if (!this.isSkillsControlActive(binding)) {
      return;
    }
    this.store.triggerHapticPreset('selection');
    this.loadSkills();
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text($r('app.string.settings_readonly_badge'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding({ left: 8, right: 8, top: 3, bottom: 3 })
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_SM)

            ForEach(
              this.skillsControlBindings('skills_scope'),
              (binding: OrbitSettingsControlBinding) => {
                Button(
                  this.includeProjectScope
                    ? $r('app.string.settings_ext_scope_project')
                    : $r('app.string.settings_ext_scope_host'),
                  { type: ButtonType.Capsule, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .accessibilityText($r('app.string.settings_ext_scope_toggle_a11y'))
                  .onClick(() => {
                    this.toggleSkillsProjectScope(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) =>
                `skills_scope_${binding.renderKey}_${this.includeProjectScope ? '1' : '0'}`
            )

            Blank()

            ForEach(
              this.skillsControlBindings('skills_refresh'),
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
                    this.refreshSkills(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) => `skills_refresh_${binding.renderKey}`
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
        } else if (this.loading && this.skills.length === 0) {
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
        } else {
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

          if (this.skills.length === 0) {
            ListItem() {
              Text($r('app.string.settings_skills_empty'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_LG)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
            }
          } else {
            ListItemGroup() {
              ForEach(
                this.skills,
                (skill: ManagedSkillRecordDto) => {
                  ListItem() {
                    Column({ space: 4 }) {
                      Row({ space: 6 }) {
                        Text(skill.name)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .layoutWeight(1)

                        Text(skill.source)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                          .backgroundColor(this.store.palette.subtleSurface)
                          .borderRadius(OrbitTokens.RADIUS_SM)
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)

                      if (skill.description.length > 0) {
                        Text(skill.description)
                          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(3)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                      }

                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Text(`/${skill.slug}`)
                          .attributeModifier(orbitTextMono(this.store.palette.accentPrimary))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        Text($r('app.string.settings_skills_tokens', skill.approx_tokens))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        Blank()
                        Text(
                          skill.enabled
                            ? $r('app.string.settings_state_enabled')
                            : $r('app.string.settings_state_off')
                        )
                          .attributeModifier(
                            orbitTextMeta(
                              skill.enabled
                                ? this.store.palette.statusDirect
                                : this.store.palette.fgMuted
                            )
                          )
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .width('100%')
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                    .alignItems(HorizontalAlign.Start)
                  }
                },
                (skill: ManagedSkillRecordDto) => `skill_${skill.id}`
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
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_skills'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.loadSkills();
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

@ComponentV2
export struct OrbitSettingsMemoryPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private selectedCwd: string = '';
  @Local private memoryBindings: OrbitMemoryRowBinding[] = [];
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private busyMemoryIds: string[] = [];
  @Local private loadedListSeq: number = 0;
  @Local private loadedHostId: string = '';
  @Local private loadedScopeKey: string = '';
  @Local private loadedRequestCwd: string = '';
  @Local private visibleEpoch: number = 1;
  private nextLoadSeq: number = 0;
  private mutationEpoch: number = 0;
  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private currentRequestCwd(): string {
    return nonBlankPath(this.selectedCwd);
  }

  private currentScopeKey(): string {
    return `${this.resolvedHostId()}\u0000${this.currentRequestCwd()}`;
  }

  private availableProjectCwds(): string[] {
    return collectHostProjectCwds(this.store, this.resolvedHostId());
  }

  private boundProjectCwds(): OrbitMemoryCwdOptionBinding[] {
    const cwds = this.availableProjectCwds();
    const targetHost = this.resolvedHostId();
    const hostEpoch = this.store.hostSelectionEpoch;
    const rEpoch = this.rootEpoch;
    const vEpoch = this.visibleEpoch;
    const out: OrbitMemoryCwdOptionBinding[] = [];
    for (let i = 0; i < cwds.length; i++) {
      out.push(createMemoryCwdOptionBinding(targetHost, hostEpoch, rEpoch, vEpoch, cwds[i]));
    }
    return out;
  }

  private memoryRefreshBindings(): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        'memory_refresh'
      )
    ];
  }

  private selectMemoryCwd(option: OrbitMemoryCwdOptionBinding): void {
    if (
      !isMemoryCwdOptionBindingActive(
        option,
        this.pageMounted,
        this.pageVisible,
        this.visibleEpoch,
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.availableProjectCwds()
      )
    ) {
      return;
    }
    if (this.selectedCwd !== option.rawCwd) {
      this.selectedCwd = option.rawCwd;
      this.clearLoadedMemoriesState();
      this.errorText = '';
      this.store.triggerHapticPreset('selection');
      this.loadMemories();
    }
  }

  private refreshMemories(binding: OrbitSettingsControlBinding): void {
    if (
      !isSettingsControlBindingActive(
        binding,
        this.pageMounted,
        this.pageVisible,
        this.visibleEpoch,
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        'memory_refresh'
      )
    ) {
      return;
    }
    this.store.triggerHapticPreset('selection');
    this.pickInitialCwdAndLoad();
  }

  private isMemoryBusy(memoryId: string): boolean {
    return this.busyMemoryIds.includes(memoryId);
  }

  private setMemoryBusy(memoryId: string, busy: boolean): void {
    const next: string[] = [];
    for (let i = 0; i < this.busyMemoryIds.length; i++) {
      if (this.busyMemoryIds[i] !== memoryId) {
        next.push(this.busyMemoryIds[i]);
      }
    }
    if (busy) {
      next.push(memoryId);
    }
    this.busyMemoryIds = next;
  }

  private clearLoadedMemoriesState(): void {
    this.guard.cancelDeleteConfirm();
    this.memoryBindings = [];
    this.loadedListSeq = 0;
    this.loadedHostId = '';
    this.loadedScopeKey = '';
    this.loadedRequestCwd = '';
    this.busyMemoryIds = [];
  }

  private isBindingActionable(binding: OrbitMemoryRowBinding): boolean {
    return isMemoryRowBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.currentScopeKey(),
      this.currentRequestCwd(),
      this.memoryBindings
    );
  }

  private canSettleMemoryBinding(binding: OrbitMemoryRowBinding): boolean {
    return canSettleMemoryRowWrite(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.loadedHostId,
      this.loadedScopeKey,
      this.loadedRequestCwd,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.currentScopeKey(),
      this.currentRequestCwd()
    );
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.pickInitialCwdAndLoad();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.clearLoadedMemoriesState();
    this.loading = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), '__reset__');
    this.clearLoadedMemoriesState();
    this.errorText = '';
    this.loading = false;
    this.selectedCwd = '';
    if (this.pageMounted && this.pageVisible) {
      this.pickInitialCwdAndLoad();
    }
  }

  private pickInitialCwdAndLoad(): void {
    this.selectedCwd = resolveEffectiveMemoryCwd(this.selectedCwd, this.availableProjectCwds());
    this.loadMemories();
  }

  private loadMemories(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const targetHost = this.resolvedHostId();
    // Preserve exact POSIX path including trailing spaces, slashes, or backslashes (never `.trim()`).
    const targetCwd = this.currentRequestCwd();
    const scopeKey = `${targetHost}\u0000${targetCwd}`;
    const capturedGlobalHostEpoch = this.store.hostSelectionEpoch;
    const capturedRootEpoch = this.rootEpoch;
    const capturedVisibleEpoch = this.visibleEpoch;
    const scopeChanged =
      this.loadedListSeq === 0 ||
      this.loadedHostId !== targetHost ||
      this.loadedScopeKey !== scopeKey ||
      this.loadedRequestCwd !== targetCwd;
    if (scopeChanged) {
      this.clearLoadedMemoriesState();
    }
    const capturedMutationEpoch = this.mutationEpoch;
    const token = this.guard.beginLoad(
      targetHost,
      scopeKey,
      capturedGlobalHostEpoch,
      capturedRootEpoch
    );
    if (targetHost.length === 0 || targetCwd.length === 0) {
      this.loading = false;
      this.clearLoadedMemoriesState();
      this.errorText = '';
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.listProjectMemoriesScoped(targetHost, targetCwd, 100).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loading = false;
      if (this.mutationEpoch !== capturedMutationEpoch) {
        return;
      }
      if (res.ok && res.data !== null) {
        this.nextLoadSeq += 1;
        const batchSeq = this.nextLoadSeq;
        const nextBindings: OrbitMemoryRowBinding[] = [];
        for (let i = 0; i < res.data.length; i++) {
          nextBindings.push({
            loadSeq: batchSeq,
            hostId: targetHost,
            hostSelectionEpoch: capturedGlobalHostEpoch,
            rootEpoch: capturedRootEpoch,
            visibleEpoch: capturedVisibleEpoch,
            scopeKey: scopeKey,
            requestCwd: targetCwd,
            item: res.data[i]
          });
        }
        this.loadedListSeq = batchSeq;
        this.loadedHostId = targetHost;
        this.loadedScopeKey = scopeKey;
        this.loadedRequestCwd = targetCwd;
        this.memoryBindings = nextBindings;
        this.errorText = '';
      } else {
        if (
          this.loadedHostId !== targetHost ||
          this.loadedScopeKey !== scopeKey ||
          this.loadedRequestCwd !== targetCwd
        ) {
          this.clearLoadedMemoriesState();
        }
        this.errorText = res.errorDetail;
      }
    });
  }

  private confirmMemory(binding: OrbitMemoryRowBinding): void {
    if (this.isMemoryBusy(binding.item.id) || !this.isBindingActionable(binding)) {
      return;
    }
    this.mutationEpoch += 1;
    const writeToken = this.guard.beginItemWrite(
      binding.hostId,
      binding.scopeKey,
      binding.item.id,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.setMemoryBusy(binding.item.id, true);
    this.errorText = '';
    this.store.confirmProjectMemoryScoped(binding.hostId, binding.item.id, binding.requestCwd).then((res) => {
      if (!this.guard.isItemWriteCurrent(writeToken, this.store.hostSelectionEpoch, this.rootEpoch)) {
        return;
      }
      if (!this.canSettleMemoryBinding(binding)) {
        return;
      }
      this.setMemoryBusy(binding.item.id, false);
      if (res.ok) {
        this.mutationEpoch += 1;
        this.store.triggerHapticPreset('success');
        this.memoryBindings = applySettledMemoryConfirm(this.memoryBindings, binding.item.id, Date.now());
      } else {
        this.store.triggerHapticPreset('warning');
        this.errorText = res.errorDetail;
      }
    });
  }

  private promptDeleteMemory(binding: OrbitMemoryRowBinding): void {
    if (this.isMemoryBusy(binding.item.id) || !this.isBindingActionable(binding)) {
      return;
    }
    // Freeze the complete immutable row binding context BEFORE displaying the dialog.
    // Both dialog copy (`binding.item.title`) and RPC target (`binding.hostId`, `binding.item.id`,
    // `binding.requestCwd`) come exclusively from this verified binding.
    const snap = this.guard.freezeDeleteConfirm(
      binding.hostId,
      binding.scopeKey,
      binding.requestCwd,
      binding.item.id,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.getUIContext().showAlertDialog({
      title: $r('app.string.settings_memory_delete_title'),
      message: $r('app.string.settings_memory_delete_message', binding.item.title),
      autoCancel: true,
      cancel: () => {
        this.guard.cancelDeleteConfirm();
      },
      primaryButton: {
        value: $r('app.string.action_cancel'),
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          this.guard.cancelDeleteConfirm();
        }
      },
      secondaryButton: {
        value: $r('app.string.action_delete'),
        fontColor: this.store.palette.statusDanger,
        action: () => {
          const liveHost = this.resolvedHostId();
          const liveCwd = this.currentRequestCwd();
          const liveScopeKey = `${liveHost}\u0000${liveCwd}`;
          // Strict pre-RPC validation against BOTH immutable row binding AND delete dialog snapshot.
          if (
            !this.isBindingActionable(binding) ||
            !this.guard.isDeleteConfirmValid(
              snap,
              liveHost,
              liveScopeKey,
              liveCwd,
              this.store.hostSelectionEpoch,
              this.rootEpoch
            ) ||
            this.isMemoryBusy(binding.item.id)
          ) {
            return;
          }
          this.mutationEpoch += 1;
          const writeToken = this.guard.beginItemWrite(
            binding.hostId,
            binding.scopeKey,
            binding.item.id,
            binding.hostSelectionEpoch,
            binding.rootEpoch
          );
          this.setMemoryBusy(binding.item.id, true);
          this.errorText = '';
          this.store.deleteProjectMemoryScoped(binding.hostId, binding.item.id, binding.requestCwd).then((res) => {
            if (!this.guard.isItemWriteCurrent(writeToken, this.store.hostSelectionEpoch, this.rootEpoch)) {
              return;
            }
            if (!this.canSettleMemoryBinding(binding)) {
              return;
            }
            this.setMemoryBusy(binding.item.id, false);
            if (res.ok) {
              this.mutationEpoch += 1;
              this.store.triggerHapticPreset('warning');
              this.memoryBindings = applySettledMemoryDelete(this.memoryBindings, binding.item.id);
            } else {
              this.store.triggerHapticPreset('warning');
              this.errorText = res.errorDetail;
            }
          });
        }
      }
    });
  }

  private statusBadgeResource(status: string): Resource {
    if (status === 'confirmed') {
      return $r('app.string.settings_memory_status_confirmed');
    }
    if (status === 'needs_revalidation') {
      return $r('app.string.settings_memory_status_recheck');
    }
    return $r('app.string.settings_memory_status_hypothesis');
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        // Project CWD picker chips
        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_memory_workspace_label'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .layoutWeight(1)
              ForEach(
                this.memoryRefreshBindings(),
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
                      this.refreshMemories(binding);
                    })
                },
                (binding: OrbitSettingsControlBinding) => `mem_refresh_${binding.renderKey}`
              )
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)

            if (this.availableProjectCwds().length > 0) {
              Scroll() {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  ForEach(
                    this.boundProjectCwds(),
                    (option: OrbitMemoryCwdOptionBinding) => {
                      Button(basenameOfPath(option.rawCwd) || option.rawCwd, {
                        type: ButtonType.Capsule,
                        stateEffect: true
                      })
                        .attributeModifier(
                          orbitButtonMeta(
                            this.selectedCwd === option.rawCwd
                              ? this.store.palette.onAccent
                              : this.store.palette.fgSecondary,
                            this.selectedCwd === option.rawCwd
                              ? this.store.palette.accentPrimaryFill
                              : this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                        .accessibilityText(option.rawCwd)
                        .onClick(() => {
                          this.selectMemoryCwd(option);
                        })
                    },
                    (option: OrbitMemoryCwdOptionBinding) => `mem_cwd_${option.renderKey}`
                  )
                }
              }
              .scrollable(ScrollDirection.Horizontal)
              .scrollBar(BarState.Off)
              .width('100%')
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
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
        } else if (this.selectedCwd.length === 0) {
          ListItem() {
            Text($r('app.string.settings_memory_no_workspace'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding(OrbitTokens.SPACE_LG)
              .width('100%')
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else if (this.loading && this.memoryBindings.length === 0) {
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
        } else {
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

          if (this.memoryBindings.length === 0) {
            ListItem() {
              Text($r('app.string.settings_memory_empty'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_LG)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
            }
          } else {
            ListItemGroup() {
              ForEach(
                this.memoryBindings,
                (binding: OrbitMemoryRowBinding) => {
                  ListItem() {
                    Column({ space: OrbitTokens.SPACE_SM }) {
                      Row({ space: 6 }) {
                        Text(binding.item.title)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(2)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .layoutWeight(1)

                        Text(this.statusBadgeResource(binding.item.status))
                          .attributeModifier(
                            orbitTextMeta(
                              binding.item.status === 'confirmed'
                                ? this.store.palette.statusDirect
                                : this.store.palette.statusWarning
                            )
                          )
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                          .backgroundColor(this.store.palette.subtleSurface)
                          .borderRadius(OrbitTokens.RADIUS_SM)
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)

                      Text(binding.item.content)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .width('100%')

                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Text(binding.item.category)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        Blank()
                        if (binding.item.status !== 'confirmed') {
                          Button($r('app.string.action_confirm'), { type: ButtonType.Capsule, stateEffect: true })
                            .attributeModifier(
                              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                            )
                            .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                            .enabled(!this.isMemoryBusy(binding.item.id) && this.isBindingActionable(binding))
                            .accessibilityText($r('app.string.settings_memory_confirm_a11y'))
                            .onClick(() => {
                              this.confirmMemory(binding);
                            })
                        }
                        Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.subtleSurface)
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                          .enabled(!this.isMemoryBusy(binding.item.id) && this.isBindingActionable(binding))
                          .accessibilityText($r('app.string.settings_memory_delete_a11y'))
                          .onClick(() => {
                            this.promptDeleteMemory(binding);
                          })
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                    .alignItems(HorizontalAlign.Start)
                  }
                },
                (binding: OrbitMemoryRowBinding) =>
                  `${binding.loadSeq}::${binding.visibleEpoch}::${binding.item.id}::${binding.item.status}`
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
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_memory'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.busyMemoryIds = [];
        this.pickInitialCwdAndLoad();
      }
    })
    .onHidden(() => {
      this.hasHiddenSinceAppear = true;
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.clearLoadedMemoriesState();
      this.loading = false;
    })
  }
}

@ComponentV2
export struct OrbitSettingsPluginsPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private pluginBindings: OrbitPluginRowBinding[] = [];
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private togglingIds: string[] = [];
  @Local private loadedListSeq: number = 0;
  @Local private loadedHostId: string = '';
  @Local private visibleEpoch: number = 1;
  private nextLoadSeq: number = 0;
  private mutationEpoch: number = 0;
  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  private clearLoadedPluginsState(): void {
    this.pluginBindings = [];
    this.loadedListSeq = 0;
    this.loadedHostId = '';
    this.togglingIds = [];
  }

  private isPluginBindingActionable(binding: OrbitPluginRowBinding): boolean {
    return isPluginRowBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch,
      this.pluginBindings
    );
  }

  private canSettlePluginBinding(binding: OrbitPluginRowBinding): boolean {
    return canSettlePluginRowWrite(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.loadedListSeq,
      this.loadedHostId,
      this.resolvedHostId(),
      this.store.hostSelectionEpoch,
      this.rootEpoch
    );
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadPlugins();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.clearLoadedPluginsState();
    this.loading = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), '__reset__');
    this.clearLoadedPluginsState();
    this.errorText = '';
    this.loading = false;
    if (this.pageMounted && this.pageVisible) {
      this.loadPlugins();
    }
  }

  private loadPlugins(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const targetHost = this.resolvedHostId();
    const capturedGlobalHostEpoch = this.store.hostSelectionEpoch;
    const capturedRootEpoch = this.rootEpoch;
    const capturedVisibleEpoch = this.visibleEpoch;
    if (this.loadedListSeq === 0 || this.loadedHostId !== targetHost) {
      this.clearLoadedPluginsState();
    }
    const capturedMutationEpoch = this.mutationEpoch;
    const token = this.guard.beginLoad(
      targetHost,
      'plugins',
      capturedGlobalHostEpoch,
      capturedRootEpoch
    );
    if (targetHost.length === 0) {
      this.loading = false;
      this.clearLoadedPluginsState();
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.listPluginsScoped(targetHost).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loading = false;
      if (this.mutationEpoch !== capturedMutationEpoch) {
        return;
      }
      if (res.ok && res.data !== null) {
        this.nextLoadSeq += 1;
        const batchSeq = this.nextLoadSeq;
        const nextBindings: OrbitPluginRowBinding[] = [];
        for (let i = 0; i < res.data.length; i++) {
          nextBindings.push({
            loadSeq: batchSeq,
            hostId: targetHost,
            hostSelectionEpoch: capturedGlobalHostEpoch,
            rootEpoch: capturedRootEpoch,
            visibleEpoch: capturedVisibleEpoch,
            plugin: res.data[i]
          });
        }
        this.loadedListSeq = batchSeq;
        this.loadedHostId = targetHost;
        this.pluginBindings = nextBindings;
        this.errorText = '';
      } else {
        if (this.loadedHostId !== targetHost) {
          this.clearLoadedPluginsState();
        }
        this.errorText = res.errorDetail;
      }
    });
  }

  private isToggling(pluginId: string): boolean {
    return this.togglingIds.includes(pluginId);
  }

  private setToggling(pluginId: string, busy: boolean): void {
    const next: string[] = [];
    for (let i = 0; i < this.togglingIds.length; i++) {
      if (this.togglingIds[i] !== pluginId) {
        next.push(this.togglingIds[i]);
      }
    }
    if (busy) {
      next.push(pluginId);
    }
    this.togglingIds = next;
  }

  private togglePlugin(binding: OrbitPluginRowBinding, nextEnabled: boolean): void {
    const pluginId = binding.plugin.manifest.id;
    if (this.isToggling(pluginId) || !this.isPluginBindingActionable(binding)) {
      return;
    }
    this.mutationEpoch += 1;
    const writeToken = this.guard.beginItemWrite(
      binding.hostId,
      'plugins',
      pluginId,
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.setToggling(pluginId, true);
    this.errorText = '';
    this.store.setPluginEnabledScoped(binding.hostId, pluginId, nextEnabled).then((res) => {
      if (!this.guard.isItemWriteCurrent(writeToken, this.store.hostSelectionEpoch, this.rootEpoch)) {
        return;
      }
      if (!this.canSettlePluginBinding(binding)) {
        return;
      }
      this.setToggling(pluginId, false);
      if (res.ok) {
        this.mutationEpoch += 1;
        this.store.triggerHapticPreset('selection');
        this.pluginBindings = applySettledPluginToggle(this.pluginBindings, pluginId, nextEnabled);
      } else {
        this.store.triggerHapticPreset('warning');
        this.errorText = res.errorDetail;
        this.loadPlugins();
      }
    });
  }

  private pluginRefreshBindings(): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        'plugins_refresh'
      )
    ];
  }

  private refreshPlugins(binding: OrbitSettingsControlBinding): void {
    if (
      !isSettingsControlBindingActive(
        binding,
        this.pageMounted,
        this.pageVisible,
        this.visibleEpoch,
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        'plugins_refresh'
      )
    ) {
      return;
    }
    this.store.triggerHapticPreset('selection');
    this.loadPlugins();
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Row() {
            Blank()
            ForEach(
              this.pluginRefreshBindings(),
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
                    this.refreshPlugins(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) => `plugins_refresh_${binding.renderKey}`
            )
          }
          .width('100%')
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
        } else if (this.loading && this.pluginBindings.length === 0) {
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
        } else {
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

          if (this.pluginBindings.length === 0) {
            ListItem() {
              Text($r('app.string.settings_plugins_empty'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
                .padding(OrbitTokens.SPACE_LG)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
            }
          } else {
            ListItemGroup() {
              ForEach(
                this.pluginBindings,
                (binding: OrbitPluginRowBinding) => {
                  ListItem() {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Column({ space: 4 }) {
                        Row({ space: 6 }) {
                          Text(binding.plugin.manifest.name)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .flexShrink(1)
                          Text(`v${binding.plugin.manifest.version}`)
                            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        }
                        .alignItems(VerticalAlign.Center)

                        if (binding.plugin.manifest.description.length > 0) {
                          Text(binding.plugin.manifest.description)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(3)
                        }

                        Text(
                          $r(
                            'app.string.settings_plugins_caps_summary',
                            binding.plugin.manifest.mcp_ops.length,
                            binding.plugin.manifest.capabilities.hook_events.length
                          )
                        )
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      Toggle({ type: ToggleType.Switch, isOn: binding.plugin.enabled })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .enabled(!this.isToggling(binding.plugin.manifest.id) && this.isPluginBindingActionable(binding))
                        .accessibilityText(binding.plugin.manifest.name)
                        .onChange((val: boolean) => {
                          if (val !== binding.plugin.enabled) {
                            this.togglePlugin(binding, val);
                          }
                        })
                    }
                    .width('100%')
                    .padding(OrbitTokens.SPACE_LG)
                    .alignItems(VerticalAlign.Center)
                  }
                },
                (binding: OrbitPluginRowBinding) =>
                  `${binding.loadSeq}::${binding.visibleEpoch}::${binding.plugin.manifest.id}::${binding.plugin.enabled ? '1' : '0'}`
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
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_plugins'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.togglingIds = [];
        this.loadPlugins();
      }
    })
    .onHidden(() => {
      this.hasHiddenSinceAppear = true;
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.clearLoadedPluginsState();
      this.loading = false;
    })
  }
}

@ComponentV2
export struct OrbitSettingsSandboxPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private policy: SandboxPolicyDto | null = null;
  @Local private loaded: boolean = false;
  @Local private loading: boolean = false;
  @Local private errorText: string = '';
  @Local private visibleEpoch: number = 1;
  private hasHiddenSinceAppear: boolean = false;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.hasHiddenSinceAppear = false;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.loadSandbox();
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
    this.guard.invalidate(this.resolvedHostId(), '__reset__');
    this.policy = null;
    this.loaded = false;
    this.errorText = '';
    this.loading = false;
    if (this.pageMounted && this.pageVisible) {
      this.loadSandbox();
    }
  }

  private loadSandbox(): void {
    if (!this.pageMounted || !this.pageVisible) {
      this.loading = false;
      return;
    }
    const targetHost = this.resolvedHostId();
    const capturedVisibleEpoch = this.visibleEpoch;
    const token = this.guard.beginLoad(
      targetHost,
      'sandbox',
      this.store.hostSelectionEpoch,
      this.rootEpoch
    );
    if (targetHost.length === 0) {
      this.loading = false;
      this.loaded = false;
      this.policy = null;
      return;
    }
    this.loading = true;
    this.errorText = '';
    this.store.getSandboxConfigScoped(targetHost).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        capturedVisibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.loading = false;
      this.loaded = true;
      if (res.ok) {
        this.policy = res.data;
        this.errorText = '';
      } else {
        this.errorText = res.errorDetail;
      }
    });
  }

  private sandboxModeResource(mode: string): Resource {
    if (mode === 'off') {
      return $r('app.string.settings_sandbox_mode_off');
    }
    if (mode === 'strict_read_only') {
      return $r('app.string.settings_sandbox_mode_readonly');
    }
    return $r('app.string.settings_sandbox_mode_workspace');
  }

  private sandboxRefreshBindings(): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        'sandbox_refresh'
      )
    ];
  }

  private refreshSandbox(binding: OrbitSettingsControlBinding): void {
    if (
      !isSettingsControlBindingActive(
        binding,
        this.pageMounted,
        this.pageVisible,
        this.visibleEpoch,
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        'sandbox_refresh'
      )
    ) {
      return;
    }
    this.store.triggerHapticPreset('selection');
    this.loadSandbox();
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text($r('app.string.settings_readonly_badge'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding({ left: 8, right: 8, top: 3, bottom: 3 })
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_SM)

            Blank()

            ForEach(
              this.sandboxRefreshBindings(),
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
                    this.refreshSandbox(binding);
                  })
              },
              (binding: OrbitSettingsControlBinding) => `sandbox_refresh_${binding.renderKey}`
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
        } else if (this.loading && !this.loaded) {
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
        } else if (this.errorText.length > 0) {
          ListItem() {
            Text(this.errorText)
              .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else if (this.policy === null) {
          ListItem() {
            Text($r('app.string.settings_sandbox_default_notice'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
          }
        } else {
          ListItemGroup() {
            ListItem() {
              Row() {
                Text($r('app.string.settings_sandbox_mode_label'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                Blank()
                Text(this.sandboxModeResource(this.policy.mode))
                  .attributeModifier(orbitTextMono(this.store.palette.accentPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
            }

            ListItem() {
              Row() {
                Text($r('app.string.settings_sandbox_network_label'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                Blank()
                Text(
                  this.policy.allow_network
                    ? $r('app.string.settings_state_enabled')
                    : $r('app.string.settings_state_off')
                )
                  .attributeModifier(
                    orbitTextMono(
                      this.policy.allow_network
                        ? this.store.palette.statusWarning
                        : this.store.palette.statusDirect
                    )
                  )
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
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

          if (this.policy.writable_roots.length > 0) {
            ListItem() {
              OrbitSectionHeader({ title: $r('app.string.settings_sandbox_writable_roots') })
            }
            ListItem() {
              Column({ space: 4 }) {
                ForEach(
                  this.policy.writable_roots,
                  (root: string) => {
                    Text(root)
                      .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  },
                  (root: string) => `wr_${root}`
                )
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          }

          if (this.policy.denied_read_paths.length > 0) {
            ListItem() {
              OrbitSectionHeader({ title: $r('app.string.settings_sandbox_denied_reads') })
            }
            ListItem() {
              Column({ space: 4 }) {
                ForEach(
                  this.policy.denied_read_paths,
                  (denied: string) => {
                    Text(denied)
                      .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  },
                  (denied: string) => `dr_${denied}`
                )
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          }
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .title($r('app.string.settings_item_sandbox'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = this.hasHiddenSinceAppear || !this.pageVisible || !this.guard.isVisible;
      this.hasHiddenSinceAppear = false;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
        this.guard.setVisible(true);
        this.loadSandbox();
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
