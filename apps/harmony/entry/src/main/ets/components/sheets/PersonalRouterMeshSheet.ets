import {
  OrbitMeshPeerRow,
  PersonalRouterConfigDto,
  RouterHostCapabilitiesDto
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitPersonalAgentStore } from '../../model/stores/OrbitPersonalAgentStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSectionHeader, OrbitSheetHeader } from '../common/OrbitCommonControls';

@ComponentV2
export struct PersonalRouterMeshSheet {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private personalAgentStore: OrbitPersonalAgentStore = OrbitHarmonyStore.getInstance().personalAgentStore;
  @Local private selectedRouterHostId: string = '';
  @Local private draftEnabled: boolean = false;
  @Local private draftScope: string = 'global_fleet';
  @Local private draftPreReview: string = 'disabled';
  @Local private draftShareHermes: boolean = true;

  aboutToAppear(): void {
    this.syncDraftFromStore();
    this.store.refreshRouterAndMesh();
  }

  @Monitor('personalAgentStore.activeRouterStatus')
  onRouterStatusUpdated(_mon: IMonitor): void {
    this.syncDraftFromStore();
  }

  private syncDraftFromStore(): void {
    const snap = this.store.activeRouterStatus;
    if (snap !== null) {
      this.selectedRouterHostId = snap.host_capabilities.host_id;
      this.draftEnabled = snap.config.enabled;
      this.draftScope = snap.config.scope;
      this.draftPreReview = snap.config.pre_review_mode;
      this.draftShareHermes = snap.config.share_host_hermes_memory;
    } else if (this.store.primaryRouterHostId.length > 0) {
      this.selectedRouterHostId = this.store.primaryRouterHostId;
    } else if (this.store.selectedHostId.length > 0) {
      this.selectedRouterHostId = this.store.selectedHostId;
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
      share_host_hermes_memory: this.draftShareHermes,
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
    Column({ space: 0 }) {
      OrbitSheetHeader({
        title: $r('app.string.router_mesh_title'),
        subtitle: `${this.store.routerHosts.length} host(s) · ${this.store.fleetMeshPeers.length} mesh link(s)`,
        onClose: () => {
          this.store.showRouterMeshSheet = false;
        }
      })

      Scroll() {
        Column({ space: OrbitTokens.SPACE_MD }) {
          // 1. Router Candidate Hosts
          OrbitSectionHeader({ title: $r('app.string.router_section_hosts'), isFirst: true })

          if (this.store.routerHosts.length === 0) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text('No router-capable hosts discovered yet.')
                .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
              Button($r('app.string.action_refresh'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                .height(34)
                .padding({ left: 14, right: 14 })
                .onClick(() => {
                  this.store.refreshRouterAndMesh();
                })
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          } else {
            Column({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                this.store.routerHosts,
                (cand: RouterHostCapabilitiesDto) => {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Row()
                        .width(8)
                        .height(8)
                        .borderRadius(4)
                        .backgroundColor(
                          cand.online ? this.store.palette.statusDirect : this.store.palette.fgMuted
                        )

                      Column({ space: 2 }) {
                        Row({ space: 6 }) {
                          Text(cand.host_name)
                            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                          if (cand.hermes_installed) {
                            Text('Hermes')
                              .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
                              .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                              .backgroundColor(this.store.palette.accentPrimarySoft)
                              .borderRadius(999)
                          }
                        }
                        Text(
                          cand.recommendation_reason !== undefined && cand.recommendation_reason !== null
                            ? cand.recommendation_reason
                            : `Score ${cand.recommended_score} · ${cand.available_providers.join(', ')}`
                        )
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)

                      if (this.selectedRouterHostId === cand.host_id) {
                        SymbolGlyph($r('sys.symbol.checkmark_circle_fill'))
                          .fontSize(16)
                          .fontColor([this.store.palette.accentPrimary])
                      }
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .onClick(() => {
                    this.selectedRouterHostId = cand.host_id;
                    this.store.loadRouterStatusForHost(cand.host_id);
                  })
                },
                (cand: RouterHostCapabilitiesDto) => `rcand_${cand.host_id}`
              )
            }
            .width('100%')
          }

          // 2. Personal Router Policy Editor
          if (this.selectedRouterHostId.length > 0) {
            OrbitSectionHeader({ title: $r('app.string.router_section_config') })

            Column({ space: OrbitTokens.SPACE_MD }) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Text($r('app.string.router_enable_toggle'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .layoutWeight(1)
                Toggle({ type: ToggleType.Switch, isOn: this.draftEnabled })
                  .selectedColor(this.store.palette.accentPrimaryFill)
                  .onChange((val: boolean) => {
                    this.draftEnabled = val;
                  })
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)

              Column({ space: OrbitTokens.SPACE_XS }) {
                Text('Routing Scope')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Button('Global Fleet', { type: ButtonType.Capsule, stateEffect: true })
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
                    .height(34)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.draftScope = 'global_fleet';
                    })
                  Button('Local Host Only', { type: ButtonType.Capsule, stateEffect: true })
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
                    .height(34)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.draftScope = 'local_host_only';
                    })
                }
                .width('100%')
              }
              .width('100%')
              .alignItems(HorizontalAlign.Start)

              Column({ space: OrbitTokens.SPACE_XS }) {
                Text('Pre-Review Policy')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Button('Off', { type: ButtonType.Capsule, stateEffect: true })
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
                    .height(32)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.draftPreReview = 'disabled';
                    })
                  Button('Medium Risk', { type: ButtonType.Capsule, stateEffect: true })
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
                    .height(32)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.draftPreReview = 'medium_risk_only';
                    })
                  Button('Assist', { type: ButtonType.Capsule, stateEffect: true })
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
                    .height(32)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.draftPreReview = 'assist_only';
                    })
                }
                .width('100%')
              }
              .width('100%')
              .alignItems(HorizontalAlign.Start)

              Row({ space: OrbitTokens.SPACE_SM }) {
                Text($r('app.string.router_share_hermes'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .layoutWeight(1)
                Toggle({ type: ToggleType.Switch, isOn: this.draftShareHermes })
                  .selectedColor(this.store.palette.accentPrimaryFill)
                  .onChange((val: boolean) => {
                    this.draftShareHermes = val;
                  })
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)

              Button($r('app.string.router_apply'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .width('100%')
                .height(OrbitTokens.HIT_COMPACT)
                .enabled(!this.store.routerMeshBusy)
                .onClick(() => {
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
          }

          // 3. Fleet Mesh Peers
          OrbitSectionHeader({ title: $r('app.string.router_section_mesh') })

          if (this.store.fleetMeshPeers.length === 0) {
            Column() {
              Text('No daemon-to-daemon mesh peers configured yet.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          } else {
            Column({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                this.store.fleetMeshPeers,
                (row: OrbitMeshPeerRow) => {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Row()
                      .width(8)
                      .height(8)
                      .borderRadius(4)
                      .backgroundColor(this.peerStatusColor(row.peer.status))

                    Column({ space: 2 }) {
                      Text(`${row.owner_host_name} → ${row.peer.label}`)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      Text(`${row.peer.status}${row.peer.hostname ? ` · ${row.peer.hostname}` : ''}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    Toggle({ type: ToggleType.Switch, isOn: row.peer.enabled })
                      .selectedColor(this.store.palette.accentPrimaryFill)
                      .onChange((nextOn: boolean) => {
                        if (nextOn !== row.peer.enabled) {
                          this.store.toggleMeshPeerEnabled(
                            row.owner_host_id,
                            row.peer.registration_id,
                            row.peer.label,
                            nextOn
                          );
                        }
                      })
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .alignItems(VerticalAlign.Center)
                },
                (row: OrbitMeshPeerRow) => `mesh_${row.owner_host_id}_${row.peer.registration_id}`
              )
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_SM,
          bottom: OrbitTokens.SPACE_XL + this.store.safeBottomVp
        })
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
