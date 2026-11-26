import { OrbitHarmonyStore } from './OrbitHarmonyStore';

export function orbitRuntime(): OrbitHarmonyStore {
  return OrbitHarmonyStore.getInstance();
}
