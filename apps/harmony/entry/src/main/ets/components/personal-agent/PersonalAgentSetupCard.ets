import { LengthMetrics } from '@kit.ArkUI';
import {
  HostCardDto,
  PersonalRouterConfigDto,
  ProviderModelSpecDto,
  RouterHostCapabilitiesDto,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitTheme } from '../../theme/OrbitTheme';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import { OrbitProviderBrandIcon } from '../composer/ModelPickerSheet';
import {
  mergePersonalAgentSetupConfig,
  normalizePersonalRouterConfig,
  selectRecommendedRouterHost
} from './PersonalAgentBriefingModel';

@ComponentV2
export struct PersonalAgentSetupCard {
  @Local private discovering: boolean = false;
  @Local private loadingHostConfig: boolean = false;
  @Local private activating: boolean = false;
  @Local private selectedHostId: string = '';
  @Local private selectedProvider: string = '';
  @Local private selectedModel: string = '';
  @Local private errorText: string = '';
  @Local private showConfigLoadError: boolean = false;
  @Local private userEditedProvider: boolean = false;
  @Local private userEditedModel: boolean = false;

  private readonly store: OrbitHarmonyStore = orbitRuntime();
  private configFetchSeq: number = 0;
  private submitSelectionSeq: number = 0;

  aboutToAppear(): void {
    this.discoverCandidates();
  }

  private discoverCandidates(): void {
    this.discovering = true;
    this.errorText = '';
    this.showConfigLoadError = false;
    this.store.refreshRouterAndMesh().then((): void => {
      this.syncDefaultSelections();
      if (this.selectedHostId.length > 0) {
        this.loadExistingConfigForHost(this.selectedHostId);
      }
    }).catch((err: Object): void => {
      this.errorText = (err as Error).message || '';
    }).finally((): void => {
      this.discovering = false;
    });
  }

  private loadExistingConfigForHost(hostId: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    this.configFetchSeq += 1;
    const reqSeq = this.configFetchSeq;
    this.loadingHostConfig = true;
    this.showConfigLoadError = false;
    this.store.fetchHostRouterConfig(cleanHost).then((rawCfg: PersonalRouterConfigDto | null): void => {
      if (reqSeq !== this.configFetchSeq || cleanHost !== this.selectedHostId) {
        return;
      }
      if (rawCfg !== null) {
        const cfg = normalizePersonalRouterConfig(rawCfg);
        const providers = this.availableProvidersForSelectedHost();
        if (!this.userEditedProvider && typeof cfg.provider === 'string' && cfg.provider.trim().length > 0) {
          if (providers.length === 0 || providers.indexOf(cfg.provider) >= 0) {
            this.selectedProvider = cfg.provider;
          }
        }
        if (!this.userEditedModel && typeof cfg.model === 'string' && cfg.model.trim().length > 0) {
          this.selectedModel = cfg.model.trim();
        }
        this.syncModelForProvider();
      }
    }).catch((err: Object): void => {
      if (reqSeq !== this.configFetchSeq || cleanHost !== this.selectedHostId) {
        return;
      }
      this.showConfigLoadError = true;
      this.errorText = (err as Error).message || '';
    }).finally((): void => {
      if (reqSeq === this.configFetchSeq) {
        this.loadingHostConfig = false;
      }
    });
  }

  private syncDefaultSelections(): void {
    const rec = selectRecommendedRouterHost(this.store.routerHosts);
    if (rec !== null && this.selectedHostId.length === 0) {
      this.selectedHostId = rec.host_id;
    } else if (this.selectedHostId.length === 0 && this.store.hosts.length > 0) {
      this.selectedHostId = this.store.selectedHostId.length > 0
        ? this.store.selectedHostId
        : this.store.hosts[0].host_id;
    }
    const providers = this.availableProvidersForSelectedHost();
    if (providers.length > 0 && providers.indexOf(this.selectedProvider) < 0) {
      this.selectedProvider = providers[0];
    } else if (this.selectedProvider.length === 0) {
      this.selectedProvider = this.store.composerProvider;
    }
    this.syncModelForProvider();
  }

  private syncModelForProvider(): void {
    const models = this.availableModelsForProvider(this.selectedProvider);
    if (this.selectedModel.length > 0) {
      let exists = false;
      for (let i = 0; i < models.length; i += 1) {
        if (models[i].id === this.selectedModel) {
          exists = true;
          break;
        }
      }
      if (!exists) {
        this.selectedModel = '';
      }
    }
  }

  private candidateHosts(): RouterHostCapabilitiesDto[] {
    if (this.store.routerHosts.length > 0) {
      return this.store.routerHosts;
    }
    const fallback: RouterHostCapabilitiesDto[] = [];
    for (let i = 0; i < this.store.hosts.length; i += 1) {
      const h: HostCardDto = this.store.hosts[i];
      const online = h.connection_state === 'online_direct' || h.connection_state === 'online_relay';
      fallback.push({
        host_id: h.host_id,
        host_name: h.display_name,
        is_local: h.is_local === true,
        online: online,
        hermes_installed: false,
        hermes_has_existing_memory: false,
        hermes_acp_ready: false,
        hermes_gateway_running: false,
        available_providers: h.available_providers.length > 0 ? h.available_providers : ['antigravity', 'claude_code', 'codex'],
        router_enabled: false,
        recommended_score: online ? 50 : 0
      });
    }
    return fallback;
  }

  private selectedCandidate(): RouterHostCapabilitiesDto | null {
    const list = this.candidateHosts();
    for (let i = 0; i < list.length; i += 1) {
      if (list[i].host_id === this.selectedHostId) {
        return list[i];
      }
    }
    return list.length > 0 ? list[0] : null;
  }

  private availableProvidersForSelectedHost(): string[] {
    const cand = this.selectedCandidate();
    if (cand !== null && cand.available_providers.length > 0) {
      return cand.available_providers;
    }
    return ['antigravity', 'claude_code', 'codex'];
  }

  private availableModelsForProvider(provider: string): ProviderModelSpecDto[] {
    const catalogs = this.store.providerCatalogsForHost(this.selectedHostId);
    const target = provider.trim().toLowerCase();
    for (let i = 0; i < catalogs.length; i += 1) {
      if (catalogs[i].provider.trim().toLowerCase() === target) {
        return catalogs[i].models;
      }
    }
    return [];
  }

  private async handleCreateAndEnable(): Promise<void> {
    const target = this.selectedCandidate();
    if (target === null || !target.online || this.activating) {
      return;
    }
    // Freeze the entire user selection at click time so concurrent edits or host switches
    // while awaiting target host config can NEVER cross-write host B's provider/model onto host A.
    const frozenHostId = target.host_id;
    const frozenProvider = this.selectedProvider;
    const frozenModel = this.selectedModel;
    const frozenActiveRouterAgentId = target.active_router_agent_id ?? '';
    this.submitSelectionSeq += 1;
    const reqSubmitSeq = this.submitSelectionSeq;

    this.activating = true;
    this.errorText = '';
    this.showConfigLoadError = false;
    try {
      // Always read & verify target host's latest existing config before writing so we NEVER
      // overwrite another host's existing scope, pre_review_mode, memory, or webhook settings.
      let existingCfg: PersonalRouterConfigDto | null = null;
      try {
        existingCfg = await this.store.fetchHostRouterConfig(frozenHostId);
      } catch (fetchErr) {
        if (reqSubmitSeq !== this.submitSelectionSeq) {
          return;
        }
        this.showConfigLoadError = true;
        this.errorText = (fetchErr as Error).message || '';
        return;
      }
      if (reqSubmitSeq !== this.submitSelectionSeq || this.selectedHostId !== frozenHostId) {
        return;
      }
      if (existingCfg === null) {
        this.showConfigLoadError = true;
        return;
      }
      const nextCfg = mergePersonalAgentSetupConfig(
        frozenProvider,
        frozenModel,
        existingCfg
      );
      const ok = await this.store.saveRouterConfig(frozenHostId, nextCfg, true);
      if (!ok || reqSubmitSeq !== this.submitSelectionSeq) {
        return;
      }
      await this.store.fleetStore.reloadHostAsync(frozenHostId);
      this.store.refreshPersonalAgentBriefing(true);
      const createdAgentId = this.store.activeRouterStatus?.config.router_agent_id ??
        this.store.briefing?.personal_agent?.agent_id ??
        frozenActiveRouterAgentId;
      if (createdAgentId.length > 0) {
        this.store.selectAgentOnHost(frozenHostId, createdAgentId, true);
      } else {
        this.store.showPersonalAgentSetup = false;
      }
    } catch (err) {
      if (reqSubmitSeq === this.submitSelectionSeq) {
        this.errorText = (err as Error).message || '';
      }
    } finally {
      if (reqSubmitSeq === this.submitSelectionSeq) {
        this.activating = false;
      }
    }
  }

  build() {
    Scroll() {
      Column() {
        Column({ space: OrbitTokens.SPACE_LG }) {
          // Header
          Column({ space: OrbitTokens.SPACE_SM }) {
            Stack({ alignContent: Alignment.Center }) {
              Image($r('app.media.orbit_agent_bot'))
                .width(18)
                .height(18)
                .objectFit(ImageFit.Contain)
                .fillColor($r('app.color.icon_primary'))
                .draggable(false)
            }
            .width(38)
            .height(38)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.recessedWell(this.store.palette))

            Text($r('app.string.briefing_setupTitle'))
              .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))

            Text($r('app.string.briefing_setupDescription'))
              .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)

          // 1. Primary Host picker
          if (this.discovering && this.candidateHosts().length === 0) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              LoadingProgress()
                .width(16)
                .height(16)
                .color(this.store.palette.iconSecondary)
              Text($r('app.string.briefing_setupDiscovering'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.recessedWell(this.store.palette))
          } else if (this.candidateHosts().length === 0) {
            Text($r('app.string.briefing_setupNoHosts'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.recessedWell(this.store.palette))
          } else {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.briefing_setupSelectHost'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))

              ForEach(
                this.candidateHosts(),
                (cand: RouterHostCapabilitiesDto) => {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Row()
                      .width(8)
                      .height(8)
                      .borderRadius(4)
                      .backgroundColor(
                        cand.online ? this.store.palette.statusDirect : this.store.palette.fgMuted
                      )
                      .flexShrink(0)

                    Column({ space: 2 }) {
                      Text(cand.host_name)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      Text(
                        cand.is_local
                          ? $r('app.string.briefing_setupTagLocal')
                          : $r('app.string.briefing_setupTagAlwaysOn')
                      )
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    if (this.selectedHostId === cand.host_id) {
                      SymbolGlyph($r('sys.symbol.checkmark_circle_fill'))
                        .fontSize(16)
                        .fontColor([this.store.palette.accentPrimary])
                    }
                  }
                  .width('100%')
                  .constraintSize({ minHeight: 46 })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 8, bottom: 8 })
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .backgroundColor(
                    this.selectedHostId === cand.host_id
                      ? this.store.palette.subtleSurface
                      : this.store.palette.recessed
                  )
                  .border({
                    width: this.selectedHostId === cand.host_id ? 1 : OrbitTokens.HAIRLINE,
                    color: this.selectedHostId === cand.host_id
                      ? this.store.palette.borderStrong
                      : this.store.palette.hairline
                  })
                  .enabled(!this.activating)
                  .opacity(this.activating ? 0.6 : 1)
                  .onClick(() => {
                    if (this.activating) {
                      return;
                    }
                    if (this.selectedHostId !== cand.host_id) {
                      this.submitSelectionSeq += 1;
                      this.selectedHostId = cand.host_id;
                      this.userEditedProvider = false;
                      this.userEditedModel = false;
                      const providers = this.availableProvidersForSelectedHost();
                      if (providers.length > 0 && providers.indexOf(this.selectedProvider) < 0) {
                        this.selectedProvider = providers[0];
                      }
                      this.syncModelForProvider();
                      this.loadExistingConfigForHost(cand.host_id);
                    }
                  })
                },
                (cand: RouterHostCapabilitiesDto): string => `pa_setup_host_${cand.host_id}`
              )
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)

            // 2. Provider picker
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.briefing_setupSelectProvider'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))

              Flex({ wrap: FlexWrap.Wrap, space: { main: LengthMetrics.vp(8), cross: LengthMetrics.vp(8) } }) {
                ForEach(
                  this.availableProvidersForSelectedHost(),
                  (prov: string) => {
                    Row({ space: 6 }) {
                      OrbitProviderBrandIcon({ provider: prov, iconSize: 13 })
                      Text(formatProviderLabel(prov))
                        .attributeModifier(
                          orbitTextMeta(
                            this.selectedProvider === prov
                              ? this.store.palette.onAccent
                              : this.store.palette.fgPrimary,
                            FontWeight.Medium
                          )
                        )
                        .maxLines(1)
                    }
                    .constraintSize({ minHeight: 34 })
                    .padding({ left: 12, right: 12, top: 6, bottom: 6 })
                    .borderRadius(OrbitTokens.RADIUS_MD)
                    .backgroundColor(
                      this.selectedProvider === prov
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.recessed
                    )
                    .enabled(!this.activating)
                    .opacity(this.activating ? 0.6 : 1)
                    .onClick(() => {
                      if (this.activating) {
                        return;
                      }
                      this.submitSelectionSeq += 1;
                      this.userEditedProvider = true;
                      this.selectedProvider = prov;
                      this.selectedModel = '';
                      this.syncModelForProvider();
                    })
                  },
                  (prov: string): string => `pa_setup_prov_${prov}`
                )
              }
              .width('100%')
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)

            // 3. Model picker
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.briefing_setupSelectModel'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))

              Flex({ wrap: FlexWrap.Wrap, space: { main: LengthMetrics.vp(8), cross: LengthMetrics.vp(8) } }) {
                Row() {
                  Text($r('app.string.briefing_setupDefaultModel'))
                    .attributeModifier(
                      orbitTextMeta(
                        this.selectedModel.length === 0
                          ? this.store.palette.onAccent
                          : this.store.palette.fgPrimary,
                        FontWeight.Medium
                      )
                    )
                    .maxLines(1)
                }
                .constraintSize({ minHeight: 32 })
                .padding({ left: 10, right: 10, top: 5, bottom: 5 })
                .borderRadius(OrbitTokens.RADIUS_SM)
                .backgroundColor(
                  this.selectedModel.length === 0
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.recessed
                )
                .enabled(!this.activating)
                .opacity(this.activating ? 0.6 : 1)
                .onClick(() => {
                  if (this.activating) {
                    return;
                  }
                  this.submitSelectionSeq += 1;
                  this.userEditedModel = true;
                  this.selectedModel = '';
                })

                ForEach(
                  this.availableModelsForProvider(this.selectedProvider).slice(0, 6),
                  (spec: ProviderModelSpecDto) => {
                    Row() {
                      Text(spec.label)
                        .attributeModifier(
                          orbitTextMeta(
                            this.selectedModel === spec.id
                              ? this.store.palette.onAccent
                              : this.store.palette.fgPrimary,
                            FontWeight.Medium
                          )
                        )
                        .maxLines(1)
                    }
                    .constraintSize({ minHeight: 32 })
                    .padding({ left: 10, right: 10, top: 5, bottom: 5 })
                    .borderRadius(OrbitTokens.RADIUS_SM)
                    .backgroundColor(
                      this.selectedModel === spec.id
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.recessed
                    )
                    .enabled(!this.activating)
                    .opacity(this.activating ? 0.6 : 1)
                    .onClick(() => {
                      if (this.activating) {
                        return;
                      }
                      this.submitSelectionSeq += 1;
                      this.userEditedModel = true;
                      this.selectedModel = spec.id;
                    })
                  },
                  (spec: ProviderModelSpecDto): string => `pa_setup_model_${spec.id}`
                )
              }
              .width('100%')
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
          }

          if (this.showConfigLoadError) {
            Column({ space: 4 }) {
              Text($r('app.string.briefing_setupLoadConfigFailed'))
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium))
                .width('100%')
              if (this.errorText.length > 0) {
                Text(this.errorText)
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                  .width('100%')
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_SM)
            .borderRadius(OrbitTokens.RADIUS_SM)
            .backgroundColor(this.store.palette.statusDangerSoft)
          } else if (this.errorText.length > 0) {
            Text(this.errorText)
              .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
              .borderRadius(OrbitTokens.RADIUS_SM)
              .backgroundColor(this.store.palette.statusDangerSoft)
          }

          // Primary + Secondary Actions
          Column({ space: OrbitTokens.SPACE_SM }) {
            Button(
              this.activating
                ? $r('app.string.briefing_setupEnabling')
                : $r('app.string.briefing_setupCreateOneClick'),
              { type: ButtonType.Normal, stateEffect: true }
            )
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .enabled(
                !this.discovering &&
                !this.loadingHostConfig &&
                !this.activating &&
                this.selectedCandidate() !== null &&
                this.selectedCandidate()?.online === true
              )
              .onClick(() => {
                this.handleCreateAndEnable();
              })

            Button($r('app.string.briefing_setupCustomSettings'), { type: ButtonType.Normal, stateEffect: true })
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.fgSecondary, Color.Transparent)
              )
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                this.store.openRouterMeshSheet();
              })
          }
          .width('100%')
        }
        .width('100%')
        .constraintSize({ maxWidth: 460 })
        .padding(OrbitTokens.SPACE_XL)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
        .alignItems(HorizontalAlign.Start)
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XL,
        bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .alignItems(HorizontalAlign.Center)
    }
    .width('100%')
    .height('100%')
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
    .backgroundColor(this.store.palette.canvas)
  }
}
