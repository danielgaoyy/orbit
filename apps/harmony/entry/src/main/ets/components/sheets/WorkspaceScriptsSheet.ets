import {
  WorkspaceScriptInfoDto,
  formatWorkspaceScriptStatusSummary
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';

/**
 * Scoped Workspace Dev Scripts Panel Sheet (PR-5).
 * Dispatches `requestWorkspaceScripts(hostId, workspaceId)` on appear and reads scripts
 * reactively from `hostDetail.workspace_scripts[workspaceId]` upon `host_changed` updates.
 */
@ComponentV2
export struct WorkspaceScriptsSheet {
  @Param hostId: string = '';
  @Param workspaceId: string = '';
  @Param title: string = '';
  @Event onClose: () => void = () => {};

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;
  private lastTargetSig: string = '';

  private targetSig(): string {
    return `${this.hostId}|${this.workspaceId}`;
  }

  aboutToAppear(): void {
    this.lastTargetSig = this.targetSig();
    this.refreshScripts();
  }

  @Monitor('hostId', 'workspaceId')
  onTargetChanged(_mon: IMonitor): void {
    const next = this.targetSig();
    if (next !== this.lastTargetSig) {
      this.lastTargetSig = next;
      this.refreshScripts();
    }
  }

  private refreshScripts(): void {
    const cleanHost = this.hostId.trim();
    const cleanWs = this.workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return;
    }
    this.store.ensureScopedHostDetailAsync(cleanHost);
    this.store.requestWorkspaceScripts(cleanHost, cleanWs);
  }

  @Computed
  get scripts(): WorkspaceScriptInfoDto[] {
    const _rev = this.fleetStore.hostDetailRevision;
    const _panelRev = this.store.panelDataRevision;
    if (_rev < 0 || _panelRev < 0) {
      // Keep reactive V2 dependency on hostDetailRevision and panelDataRevision
    }
    return this.store.getWorkspaceScripts(this.hostId, this.workspaceId);
  }

  private statusDotColor(status: string): ResourceColor {
    const lower = status.toLowerCase();
    if (lower === 'running') {
      return this.store.palette.statusDirect;
    }
    if (lower === 'failed') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private statusLabelResource(status: string): Resource {
    const lower = status.toLowerCase();
    if (lower === 'running') {
      return $r('app.string.scripts_status_running');
    }
    if (lower === 'failed') {
      return $r('app.string.scripts_status_failed');
    }
    return $r('app.string.scripts_status_stopped');
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.scripts_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(this.title.length > 0 ? this.title : this.workspaceId)
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.action_refresh'))
        .onClick(() => {
          this.refreshScripts();
        })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.action_close'))
        .onClick(() => {
          this.onClose();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: OrbitTokens.SPACE_MD })
      .alignItems(VerticalAlign.Center)

      if (this.scripts.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.code_square'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text($r('app.string.scripts_empty'))
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
            .textAlign(TextAlign.Center)
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.scripts,
            (script: WorkspaceScriptInfoDto) => {
              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Row()
                    .width(9)
                    .height(9)
                    .borderRadius(4.5)
                    .backgroundColor(this.statusDotColor(script.status))
                    .flexShrink(0)

                  Column({ space: 2 }) {
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Text(script.name)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .flexShrink(1)
                      Text(this.statusLabelResource(script.status))
                        .attributeModifier(orbitTextMeta(this.statusDotColor(script.status), FontWeight.Medium))
                        .maxLines(1)
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)

                    Text(formatWorkspaceScriptStatusSummary(script))
                      .attributeModifier(
                        orbitTextMono(
                          (script.last_error ?? '').length > 0
                            ? this.store.palette.statusDanger
                            : this.store.palette.fgMuted
                        )
                      )
                      .accessibilityDescription($r('app.string.scripts_status_summary_label'))
                      .maxLines(2)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                      .width('100%')
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  if (script.status.toLowerCase() === 'running') {
                    Button($r('app.string.scripts_stop'), { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                      )
                      .height(30)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.stopWorkspaceScript(this.hostId, this.workspaceId, script.name);
                      })
                  } else {
                    Button($r('app.string.scripts_start'), { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .height(30)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.startWorkspaceScript(this.hostId, this.workspaceId, script.name);
                      })
                  }
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                .alignItems(VerticalAlign.Center)
              }
            },
            (script: WorkspaceScriptInfoDto): string =>
              `ws_script_${this.workspaceId}_${script.name}_${script.status}_${script.last_error ?? ''}`
          )
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
