import { useMemo } from "react";
import { DEFAULT_LOCALE, type MessageKey } from "@/lib/i18n";
import { SHORTCUT_ACTIONS, getEffectiveChord } from "@/lib/shortcut-registry";
import { orbitActions, type OrbitUiState, useOrbitSelector } from "@/state/orbit-store";
import { preferenceDefaults as defaults } from "@/state/preference-defaults";

interface ResettableSetting {
  id: MessageKey;
  modified: (state: OrbitUiState) => boolean;
  reset: () => void;
}

function preference<K extends keyof typeof defaults>(
  id: MessageKey,
  field: K,
  apply: (value: (typeof defaults)[K]) => void,
): ResettableSetting {
  return {
    id,
    modified: (state) => state[field] !== defaults[field],
    reset: () => apply(defaults[field]),
  };
}

const resettable: readonly ResettableSetting[] = [
  preference("settings.sendWithEnter", "enterToSend", orbitActions.setEnterToSend),
  preference("settings.groupTheme", "theme", orbitActions.setTheme),
  preference("settings.diffLayout", "diffLayout", orbitActions.setDiffLayout),
  preference("settings.externalEditor", "preferredEditor", orbitActions.setPreferredEditor),
  preference("voice.microphoneTitle", "voiceInputDeviceId", orbitActions.setVoiceInputDeviceId),
  preference("settings.defaultPermissions", "draftMode", (mode) => orbitActions.setDraftConfig({ mode })),
  preference("im.defaultProvider", "draftProvider", (provider) => orbitActions.setDraftConfig({ provider })),
  preference("settings.isolateWorktrees", "draftUseWorktree", (useWorktree) => orbitActions.setDraftConfig({ useWorktree })),
  preference("settings.branchPrefix", "worktreeBranchPrefix", (worktreeBranchPrefix) => orbitActions.setWorktreePrefs({ worktreeBranchPrefix })),
  preference("settings.collapseExploration", "coalesceExploration", (coalesceExploration) => orbitActions.setWorktreePrefs({ coalesceExploration })),
  preference("settings.notifyApproval", "notifyOnApproval", (notifyOnApproval) => orbitActions.setNotificationPrefs({ notifyOnApproval })),
  preference("settings.notifyTurnComplete", "notifyOnTurnComplete", (notifyOnTurnComplete) => orbitActions.setNotificationPrefs({ notifyOnTurnComplete })),
  preference("settings.notifyAgentError", "notifyOnAgentError", (notifyOnAgentError) => orbitActions.setNotificationPrefs({ notifyOnAgentError })),
  preference("settings.notifyGoalFinished", "notifyOnGoalFinished", (notifyOnGoalFinished) => orbitActions.setNotificationPrefs({ notifyOnGoalFinished })),
  preference("settings.notificationSoundTitle", "notifySoundEnabled", (notifySoundEnabled) => orbitActions.setNotificationPrefs({ notifySoundEnabled })),
  {
    id: "settings.notifyQuietHours",
    modified: (s) => s.notifyQuietHoursEnabled !== defaults.notifyQuietHoursEnabled ||
      s.notifyQuietStartHour !== defaults.notifyQuietStartHour || s.notifyQuietEndHour !== defaults.notifyQuietEndHour,
    reset: () => orbitActions.setNotificationPrefs({
      notifyQuietHoursEnabled: defaults.notifyQuietHoursEnabled,
      notifyQuietStartHour: defaults.notifyQuietStartHour,
      notifyQuietEndHour: defaults.notifyQuietEndHour,
    }),
  },
  {
    id: "settings.language",
    modified: (s) => s.locale !== DEFAULT_LOCALE,
    reset: () => orbitActions.setLocale(DEFAULT_LOCALE),
  },
  ...SHORTCUT_ACTIONS.map((action): ResettableSetting => ({
    id: action.labelKey,
    modified: (s) => getEffectiveChord(action.id, s.customKeybindings) !== action.defaultChord,
    reset: () => orbitActions.resetCustomKeybinding(action.id),
  })),
];

function reset(id: string) {
  resettable.find((item) => item.id === id)?.reset();
}

function resetAll() {
  const snapshot = orbitActions.getSnapshot();
  for (const item of resettable) {
    if (item.modified(snapshot)) item.reset();
  }
}

// A primitive snapshot keeps React 19 subscriptions stable across unrelated store updates.
function getModifiedIds(state: OrbitUiState) {
  return resettable.filter((item) => item.modified(state)).map((item) => item.id).join(" ");
}

export function useModifiedSettings() {
  const ids = useOrbitSelector(getModifiedIds);
  return useMemo(() => ({
    modifiedIds: new Set<string>(ids ? ids.split(" ") : []),
    reset,
    resetAll,
  }), [ids]);
}
