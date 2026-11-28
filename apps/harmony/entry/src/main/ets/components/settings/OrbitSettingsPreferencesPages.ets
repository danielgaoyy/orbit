import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { PushPreferencesDto, normalizeModeSlug } from '../../model/OrbitTypes';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitSectionHeader } from '../common/OrbitCommonControls';
import {
  ORBIT_ACTIVE_TURN_SEND_MODES,
  ORBIT_APPEARANCE_MODES,
  ORBIT_LANGUAGE_MODES,
  OrbitSettingsControlBinding,
  OrbitSettingsRequestGuard,
  createSettingsControlBinding,
  isSettingsControlBindingActive,
  resolveOrbitAppearanceIndex
} from './OrbitSettingsNavModel';

const DEFAULT_PERMISSION_MODES: string[] = ['default', 'auto', 'accept_edits', 'full_access'];

interface GeneralOptionBinding {
  value: string;
  control: OrbitSettingsControlBinding;
}

@ComponentV2
export struct OrbitSettingsGeneralPage {
  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private statusText: string = '';
  @Local private statusIsError: boolean = false;
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  private generalBinding(scopeKey: string): OrbitSettingsControlBinding {
    return createSettingsControlBinding(
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      this.visibleEpoch,
      scopeKey
    );
  }

  private isGeneralControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      binding.scopeKey
    );
  }

  @Computed
  get boundLanguages(): GeneralOptionBinding[] {
    const control = this.generalBinding('general_language');
    const out: GeneralOptionBinding[] = [];
    for (let i = 0; i < ORBIT_LANGUAGE_MODES.length; i++) {
      out.push({ value: ORBIT_LANGUAGE_MODES[i], control: control });
    }
    return out;
  }

  @Computed
  get boundSendModes(): GeneralOptionBinding[] {
    const control = this.generalBinding('general_send_behavior');
    const out: GeneralOptionBinding[] = [];
    for (let i = 0; i < ORBIT_ACTIVE_TURN_SEND_MODES.length; i++) {
      out.push({ value: ORBIT_ACTIVE_TURN_SEND_MODES[i], control: control });
    }
    return out;
  }

  @Computed
  get boundPermissionModes(): GeneralOptionBinding[] {
    const control = this.generalBinding('general_default_mode');
    const out: GeneralOptionBinding[] = [];
    for (let i = 0; i < DEFAULT_PERMISSION_MODES.length; i++) {
      out.push({ value: DEFAULT_PERMISSION_MODES[i], control: control });
    }
    return out;
  }

  private languageLabel(code: string): Resource {
    if (code === 'en') {
      return $r('app.string.settings_lang_en');
    }
    if (code === 'zh_CN') {
      return $r('app.string.settings_lang_zh');
    }
    if (code === 'ja_JP') {
      return $r('app.string.settings_lang_ja');
    }
    return $r('app.string.settings_lang_system');
  }

  private sendBehaviorLabel(mode: string): Resource {
    if (mode === 'steer') {
      return $r('app.string.settings_send_steer');
    }
    if (mode === 'interrupt') {
      return $r('app.string.settings_send_interrupt');
    }
    return $r('app.string.settings_send_queue');
  }

  private sendBehaviorDesc(mode: string): Resource {
    if (mode === 'steer') {
      return $r('app.string.settings_send_steer_desc');
    }
    if (mode === 'interrupt') {
      return $r('app.string.settings_send_interrupt_desc');
    }
    return $r('app.string.settings_send_queue_desc');
  }

  private permissionModeLabel(mode: string): Resource {
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
      List({ space: 0 }) {
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_general_language_section'), isFirst: true })
        }

        ListItemGroup() {
          ForEach(
            this.boundLanguages,
            (row: GeneralOptionBinding) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text(this.languageLabel(row.value))
                      .attributeModifier(
                        orbitTextSubhead(
                          this.store.appLanguage === row.value
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgPrimary,
                          this.store.appLanguage === row.value ? FontWeight.Medium : FontWeight.Regular
                        )
                      )
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      .layoutWeight(1)
                      .textAlign(TextAlign.Start)

                    if (this.store.appLanguage === row.value) {
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
                .accessibilityText(this.languageLabel(row.value))
                .onClick(() => {
                  if (!this.isGeneralControlActive(row.control)) {
                    return;
                  }
                  this.store.setAppLanguage(row.value);
                  this.statusText = '';
                  this.statusIsError = false;
                })
              }
            },
            (row: GeneralOptionBinding) => `lang_${row.control.renderKey}_${row.value}`
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

        ListItem() {
          Text($r('app.string.settings_general_language_hint'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_XS, top: OrbitTokens.SPACE_XS })
        }

        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_general_send_section') })
        }

        ListItemGroup() {
          ForEach(
            this.boundSendModes,
            (row: GeneralOptionBinding) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Column({ space: 2 }) {
                      Text(this.sendBehaviorLabel(row.value))
                        .attributeModifier(
                          orbitTextSubhead(
                            this.store.activeTurnSendBehavior === row.value
                              ? this.store.palette.accentPrimary
                              : this.store.palette.fgPrimary,
                            FontWeight.Medium
                          )
                        )
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      Text(this.sendBehaviorDesc(row.value))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    if (this.store.activeTurnSendBehavior === row.value) {
                      SymbolGlyph($r('sys.symbol.checkmark'))
                        .fontSize(15)
                        .fontColor([this.store.palette.accentPrimary])
                    }
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)
                }
                .width('100%')
                .constraintSize({ minHeight: 56 })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .backgroundColor(Color.Transparent)
                .accessibilityText(this.sendBehaviorLabel(row.value))
                .onClick(() => {
                  if (!this.isGeneralControlActive(row.control)) {
                    return;
                  }
                  this.store.setActiveTurnSendBehavior(row.value);
                })
              }
            },
            (row: GeneralOptionBinding) => `send_${row.control.renderKey}_${row.value}`
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

        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_general_default_mode_section') })
        }

        ListItemGroup() {
          ForEach(
            this.boundPermissionModes,
            (row: GeneralOptionBinding) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text(this.permissionModeLabel(row.value))
                      .attributeModifier(
                        orbitTextSubhead(
                          normalizeModeSlug(this.store.composerMode) === row.value
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgPrimary,
                          normalizeModeSlug(this.store.composerMode) === row.value
                            ? FontWeight.Medium
                            : FontWeight.Regular
                        )
                      )
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      .layoutWeight(1)
                      .textAlign(TextAlign.Start)

                    if (normalizeModeSlug(this.store.composerMode) === row.value) {
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
                .accessibilityText(this.permissionModeLabel(row.value))
                .onClick(() => {
                  if (!this.isGeneralControlActive(row.control)) {
                    return;
                  }
                  const res = this.store.setAccountDefaultPermissionMode(row.value);
                  if (!this.isGeneralControlActive(row.control)) {
                    return;
                  }
                  if (res.ok) {
                    this.store.triggerHaptic('selection');
                    this.statusText = '';
                    this.statusIsError = false;
                  } else {
                    this.store.triggerHaptic('warning');
                    this.statusText = res.errorDetail;
                    this.statusIsError = true;
                  }
                })
              }
            },
            (row: GeneralOptionBinding) => `defmode_${row.control.renderKey}_${row.value}`
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

        if (this.statusText.length > 0) {
          ListItem() {
            Text(this.statusText)
              .attributeModifier(
                orbitTextMeta(
                  this.statusIsError ? this.store.palette.statusDanger : this.store.palette.fgSecondary
                )
              )
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .padding({ left: OrbitTokens.SPACE_XS, top: OrbitTokens.SPACE_SM })
          }
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
    .title($r('app.string.settings_item_general'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsAppearancePage {
  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;
  @Local private appearanceItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: $r('app.string.settings_appearance_system') },
    { text: $r('app.string.settings_appearance_light') },
    { text: $r('app.string.settings_appearance_dark') }
  ]);

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  private appearanceControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.store.selectedHostId.trim(),
        this.store.hostSelectionEpoch,
        0,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isAppearanceControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      binding.scopeKey
    );
  }

  private selectAppearanceMode(binding: OrbitSettingsControlBinding, idx: number): void {
    if (!this.isAppearanceControlActive(binding)) {
      return;
    }
    if (idx >= 0 && idx < ORBIT_APPEARANCE_MODES.length) {
      this.store.setThemeMode(ORBIT_APPEARANCE_MODES[idx]);
    }
  }

  @Computed
  get appearanceIndex(): number {
    return resolveOrbitAppearanceIndex(this.store.themeMode);
  }

  build() {
    NavDestination() {
      List({ space: 0 }) {
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_appearance_theme_section'), isFirst: true })
        }

        ListItem() {
          Column({ space: OrbitTokens.SPACE_MD }) {
            ForEach(
              this.appearanceControlBindings('appearance_theme'),
              (ctrl: OrbitSettingsControlBinding) => {
                CapsuleSegmentButtonV2({
                  items: this.appearanceItems,
                  selectedIndex: this.appearanceIndex,
                  onItemClicked: (idx: number) => {
                    this.selectAppearanceMode(ctrl, idx);
                  },
                  itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
                  itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
                  itemMaxFontScale: OrbitTokens.MAX_FONT_SCALE_CHROME,
                  itemMinHeight: LengthMetrics.vp(38)
                })
                  .width('100%')
              },
              (ctrl: OrbitSettingsControlBinding) => `app_theme_${ctrl.renderKey}`
            )

            Text($r('app.string.settings_appearance_theme_hint'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .width('100%')
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }

        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_appearance_a11y_section') })
        }

        ListItemGroup() {
          ListItem() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_appearance_reduced_motion'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .layoutWeight(1)
              Text(
                this.store.shellStore.reducedMotion
                  ? $r('app.string.settings_state_enabled')
                  : $r('app.string.settings_state_off')
              )
                .attributeModifier(
                  orbitTextMono(
                    this.store.shellStore.reducedMotion
                      ? this.store.palette.accentPrimary
                      : this.store.palette.fgMuted
                  )
                )
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({
              left: OrbitTokens.SPACE_LG,
              right: OrbitTokens.SPACE_LG,
              top: OrbitTokens.SPACE_SM,
              bottom: OrbitTokens.SPACE_SM
            })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_appearance_font_scale_cap'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .layoutWeight(1)
              Text(`${OrbitTokens.MAX_FONT_SCALE_CHROME}x`)
                .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({
              left: OrbitTokens.SPACE_LG,
              right: OrbitTokens.SPACE_LG,
              top: OrbitTokens.SPACE_SM,
              bottom: OrbitTokens.SPACE_SM
            })
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
    .title($r('app.string.settings_item_appearance'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsNotificationsPage {
  @Param hostId: string = '';
  @Param rootEpoch: number = 0;

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly guard: OrbitSettingsRequestGuard = new OrbitSettingsRequestGuard();
  @Local private testingPush: boolean = false;
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  private resolvedHostId(): string {
    return this.hostId.trim().length > 0 ? this.hostId.trim() : this.store.selectedHostId.trim();
  }

  @Computed
  get testPushBindings(): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        'notifications_test'
      )
    ];
  }

  @Computed
  get pushPrefBindings(): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.resolvedHostId(),
        this.store.hostSelectionEpoch,
        this.rootEpoch,
        this.visibleEpoch,
        'notifications_prefs'
      )
    ];
  }

  private isActionBindingActive(binding: OrbitSettingsControlBinding): boolean {
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

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.guard.setVisible(true);
    this.store.probeNotificationPermission();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
    this.guard.unmount();
    this.testingPush = false;
  }

  @Monitor('hostId', 'rootEpoch', 'store.hostSelectionEpoch')
  onHostContextChanged(_mon: IMonitor): void {
    this.guard.invalidate(this.resolvedHostId(), 'notifications');
    this.testingPush = false;
  }

  private runTestPush(binding: OrbitSettingsControlBinding): void {
    if (!this.isActionBindingActive(binding) || this.testingPush) {
      return;
    }
    const targetHost = binding.hostId;
    if (targetHost.length === 0) {
      this.store.showToastResource($r('app.string.toast_connect_host_first'), true);
      this.store.triggerHapticPreset('warning');
      return;
    }
    const token = this.guard.beginLoad(
      targetHost,
      'notifications_test',
      binding.hostSelectionEpoch,
      binding.rootEpoch
    );
    this.testingPush = true;
    this.store.triggerHapticPreset('action');
    this.store.sendTestPushNotificationScoped(targetHost).then((res) => {
      if (
        !this.pageMounted ||
        !this.pageVisible ||
        binding.visibleEpoch !== this.visibleEpoch ||
        !this.guard.isCurrent(token, this.store.hostSelectionEpoch, this.rootEpoch)
      ) {
        return;
      }
      this.testingPush = false;
      if (res.ok) {
        this.store.showToastResource($r('app.string.toast_test_push_sent'), false);
        this.store.triggerHapticPreset('success');
      } else {
        this.store.showToastResource($r('app.string.toast_test_push_failed'), true, res.errorDetail);
        this.store.triggerHapticPreset('warning');
      }
    });
  }

  private updatePushPrefField(binding: OrbitSettingsControlBinding, field: string, nextVal: boolean): void {
    if (!this.isActionBindingActive(binding)) {
      return;
    }
    const cur = this.store.pushPrefs;
    const updated: PushPreferencesDto = {
      enabled: field === 'enabled' ? nextVal : cur.enabled,
      notify_permission_required: field === 'permissions' ? nextVal : cur.notify_permission_required,
      notify_turn_completed: field === 'turns' ? nextVal : cur.notify_turn_completed,
      notify_agent_error: field === 'errors' ? nextVal : cur.notify_agent_error,
      notify_goal_finished: field === 'goals' ? nextVal : cur.notify_goal_finished,
      notify_timer_fired: field === 'timers' ? nextVal : cur.notify_timer_fired,
      include_message_preview: field === 'preview' ? nextVal : cur.include_message_preview
    };
    this.store.triggerHapticPreset('selection');
    this.store.updatePushPreferences(updated);
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_push_token_label'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .layoutWeight(1)
              if (this.store.pushToken.length > 0) {
                Text(`${this.store.pushToken.substring(0, 12)}…`)
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              } else {
                Text($r('app.string.settings_push_not_registered'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
            }
            .width('100%')

            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_push_synced_daemons'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .layoutWeight(1)
              Text(`${this.store.pushSyncedHostIds.length} / ${this.store.hosts.length}`)
                .attributeModifier(
                  orbitTextMono(
                    this.store.pushSyncedHostIds.length > 0
                      ? this.store.palette.statusDirect
                      : this.store.palette.fgMuted
                  )
                )
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')

            if (this.store.pushStatusMessage.length > 0) {
              Text(this.store.pushStatusMessage)
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')
            }

            ForEach(
              this.testPushBindings,
              (binding: OrbitSettingsControlBinding) => {
                Column({ space: OrbitTokens.SPACE_SM }) {
                  Button($r('app.string.push_sync_hosts'), { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(
                      orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                    )
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .borderRadius(OrbitTokens.RADIUS_MD)
                    .accessibilityText($r('app.string.push_sync_hosts'))
                    .onClick(() => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      this.store.triggerHaptic('action');
                      if (this.store.pushToken.length === 0 || !this.store.notificationsEnabled) {
                        this.store.configurePushAndNotifications(this.getUIContext().getHostContext());
                      } else {
                        this.store.syncPushTokenToAllHosts(true);
                      }
                    })

                  Button($r('app.string.push_test_button'), { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(
                      orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                    )
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .borderRadius(OrbitTokens.RADIUS_MD)
                    .enabled(!this.testingPush)
                    .accessibilityText($r('app.string.push_test_button'))
                    .onClick(() => {
                      this.runTestPush(binding);
                    })
                }
                .width('100%')
              },
              (binding: OrbitSettingsControlBinding) => `push_actions_${binding.renderKey}`
            )
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }

        ForEach(
          this.pushPrefBindings,
          (binding: OrbitSettingsControlBinding) => {
            ListItemGroup() {
              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.settings_push_master_switch'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.enabled })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .accessibilityText($r('app.string.settings_push_master_switch'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.enabled) {
                        this.updatePushPrefField(binding, 'enabled', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.push_pref_permissions'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.notify_permission_required })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.push_pref_permissions'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.notify_permission_required) {
                        this.updatePushPrefField(binding, 'permissions', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.push_pref_turns'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.notify_turn_completed })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.push_pref_turns'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.notify_turn_completed) {
                        this.updatePushPrefField(binding, 'turns', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.push_pref_errors'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.notify_agent_error })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.push_pref_errors'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.notify_agent_error) {
                        this.updatePushPrefField(binding, 'errors', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.push_pref_goals'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.notify_goal_finished })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.push_pref_goals'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.notify_goal_finished) {
                        this.updatePushPrefField(binding, 'goals', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.settings_push_pref_timers'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.notify_timer_fired })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.settings_push_pref_timers'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.notify_timer_fired) {
                        this.updatePushPrefField(binding, 'timers', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .alignItems(VerticalAlign.Center)
              }

              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text($r('app.string.push_pref_preview'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .layoutWeight(1)
                  Toggle({ type: ToggleType.Switch, isOn: this.store.pushPrefs.include_message_preview })
                    .selectedColor(this.store.palette.accentPrimaryFill)
                    .enabled(this.store.pushPrefs.enabled)
                    .accessibilityText($r('app.string.push_pref_preview'))
                    .onChange((val: boolean) => {
                      if (!this.isActionBindingActive(binding)) {
                        return;
                      }
                      if (val !== this.store.pushPrefs.include_message_preview) {
                        this.updatePushPrefField(binding, 'preview', val);
                      }
                    })
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
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
          },
          (binding: OrbitSettingsControlBinding) => `push_prefs_${binding.renderKey}`
        )
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
    .title($r('app.string.settings_item_notifications'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible || !this.guard.isVisible;
      this.pageVisible = true;
      this.guard.setVisible(true);
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
      this.guard.setVisible(false);
      this.testingPush = false;
    })
  }
}
