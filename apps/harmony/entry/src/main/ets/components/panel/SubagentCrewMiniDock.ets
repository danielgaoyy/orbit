import {
  SessionSubagentEntryDto,
  isSubagentActiveStatus
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitTokens,
  orbitTextMono
} from '../../theme/OrbitTokens';

/**
 * TopBar Crew MiniDock capsule aligned with Desktop's SubagentCrewMiniDock.
 * Displays active/total subagent counts for the bound `(hostId, rootAgentId)` session
 * and opens the Crew management list in the Right Side Panel on tap.
 * Strictly reads observable caches (zero synchronous N-API in `build()` / `@Computed`).
 */
@ComponentV2
export struct SubagentCrewMiniDock {
  @Param hostId: string = '';
  @Param rootAgentId: string = '';

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    if (this.hostId.length > 0) {
      this.store.ensureScopedHostDetailAsync(this.hostId);
    }
  }

  @Monitor('hostId')
  onHostIdChanged(_mon: IMonitor): void {
    if (this.hostId.length > 0) {
      this.store.ensureScopedHostDetailAsync(this.hostId);
    }
  }

  @Computed
  get crewEntries(): SessionSubagentEntryDto[] {
    const _rev =
      this.store.stageRevision +
      this.store.panelDataRevision +
      this.store.fleetStore.hostDetailRevision +
      this.store.archivedSubagentKeys.length;
    if (_rev < 0 || this.hostId.length === 0 || this.rootAgentId.length === 0) {
      return [];
    }
    return this.store.getScopedSubagentsForSession(this.hostId, this.rootAgentId);
  }

  @Computed
  get activeCount(): number {
    let count = 0;
    const list = this.crewEntries;
    for (let i = 0; i < list.length; i++) {
      if (isSubagentActiveStatus(list[i].status)) {
        count += 1;
      }
    }
    return count;
  }

  @Computed
  get hasAwaitingApproval(): boolean {
    const list = this.crewEntries;
    for (let i = 0; i < list.length; i++) {
      if (list[i].status.toLowerCase() === 'awaiting_permission') {
        return true;
      }
    }
    return false;
  }

  @Computed
  get statusDotColor(): ResourceColor {
    if (this.hasAwaitingApproval) {
      return this.store.palette.statusWarning;
    }
    if (this.activeCount > 0) {
      return this.store.palette.accentPrimary;
    }
    return this.store.palette.statusDirect;
  }

  @Computed
  get badgeText(): string {
    const total = this.crewEntries.length;
    if (this.activeCount > 0) {
      return `${this.activeCount}/${total}`;
    }
    return `${total}`;
  }

  build() {
    if (this.crewEntries.length > 0) {
      Button({ type: ButtonType.Capsule, stateEffect: true }) {
        Row({ space: 5 }) {
          Row()
            .width(OrbitTokens.DOT_SIZE)
            .height(OrbitTokens.DOT_SIZE)
            .borderRadius(3)
            .backgroundColor(this.statusDotColor)

          SymbolGlyph($r('sys.symbol.person_2'))
            .fontSize(13)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])

          Text(this.badgeText)
            .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
            .maxLines(1)
        }
        .alignItems(VerticalAlign.Center)
      }
      .height(OrbitTokens.HIT_COMPACT - 6)
      .padding({ left: 10, right: 10 })
      .backgroundColor(
        this.hasAwaitingApproval
          ? this.store.palette.statusWarningSoft
          : this.store.palette.subtleSurface
      )
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.hasAwaitingApproval
          ? this.store.palette.statusWarning
          : this.store.palette.hairline
      })
      .accessibilityText(`Subagent crew: ${this.activeCount} active of ${this.crewEntries.length}`)
      .onClick(() => {
        this.store.openSubagentCrewPanel(this.hostId, this.rootAgentId);
      })
    }
  }
}
