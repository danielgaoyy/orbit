import { OrbitSettingsRoot } from '../settings/OrbitSettingsRoot';

/**
 * Legacy compatibility alias delegating directly to the PR-6 four-group
 * `Navigation` + `NavPathStack` Settings architecture (`OrbitSettingsRoot`).
 */
@ComponentV2
export struct HostSettingsSheet {
  @Param initialDestination: string = '';
  @Param initialHostId: string = '';

  build() {
    OrbitSettingsRoot({
      initialDestination: this.initialDestination,
      initialHostId: this.initialHostId
    })
  }
}
