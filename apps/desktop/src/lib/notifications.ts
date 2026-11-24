import {
  type NativeApprovalNotificationRequest,
  isTauriRuntime,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { type NotificationSoundTone, playSoftNotificationChime } from "@/lib/notification-sound";

export interface DesktopNotificationDispatchOptions {
  title: string;
  body: string;
  hostId: string;
  agentId?: string | null;
  tag?: string;
  tone?: NotificationSoundTone;
  forceEvenIfVisible?: boolean;
  /** Optional permission context for interactive OS-native Approve / Deny notifications. */
  permissionRequestId?: string | null;
  isPlanApproval?: boolean;
  humanOnly?: boolean;
  riskLevel?: string | null;
  allowDecision?: string | null;
  denyDecision?: string | null;
}

export interface DesktopNotificationPolicy {
  soundEnabled: boolean;
  inQuietHours: boolean;
  onFocusTarget: (hostId: string, agentId?: string | null) => void;
}

function sendWebNotification(
  opts: DesktopNotificationDispatchOptions,
  policy: DesktopNotificationPolicy,
  chimePlayed: boolean,
): boolean {
  if (typeof window === "undefined" || !("Notification" in window)) return false;
  if (Notification.permission !== "granted") return false;
  try {
    const notif = new Notification(opts.title, {
      body: opts.body,
      silent: chimePlayed || !policy.soundEnabled,
      tag: opts.tag,
    });
    notif.onclick = () => {
      void orbitIpc.focusMainWindow().catch(() => undefined);
      if (typeof window !== "undefined") {
        window.focus();
      }
      policy.onFocusTarget(opts.hostId, opts.agentId);
      notif.close();
    };
    return true;
  } catch {
    return false;
  }
}

/**
 * Dispatches a desktop notification, preferring Tauri's native OS interactive notification
 * pipeline when running inside the desktop runtime (so standard permission gates can be
 * approved/denied directly from OS notifications without stealing window focus, while
 * `PlanApproval` and `human_only` gates strictly show "Open to Review" only).
 * Falls back gracefully to Web Notification API when running in a bare browser or if native
 * OS delivery fails.
 */
export function dispatchDesktopNotificationWithNativeFallback(
  opts: DesktopNotificationDispatchOptions,
  policy: DesktopNotificationPolicy,
): boolean {
  if (!opts.forceEvenIfVisible) {
    if (typeof document !== "undefined" && !document.hidden && document.hasFocus()) {
      return false;
    }
    if (policy.inQuietHours) {
      return false;
    }
  }

  const shouldPlaySound =
    policy.soundEnabled && (opts.forceEvenIfVisible || !policy.inQuietHours);
  const chimePlayed = shouldPlaySound
    ? playSoftNotificationChime(opts.tone ?? "default")
    : false;

  if (isTauriRuntime() && opts.hostId && opts.agentId) {
    const req: NativeApprovalNotificationRequest = {
      host_id: opts.hostId,
      agent_id: opts.agentId,
      permission_request_id: opts.permissionRequestId ?? null,
      title: opts.title,
      body: opts.body,
      is_plan_approval: Boolean(opts.isPlanApproval),
      human_only: Boolean(opts.humanOnly),
      risk_level: opts.riskLevel ?? null,
      allow_decision: opts.allowDecision ?? null,
      deny_decision: opts.denyDecision ?? null,
    };

    void orbitIpc
      .sendNativeApprovalNotification(req)
      .then((res) => {
        if (!res.delivered) {
          sendWebNotification(opts, policy, chimePlayed);
        }
      })
      .catch(() => {
        sendWebNotification(opts, policy, chimePlayed);
      });
    return true;
  }

  return sendWebNotification(opts, policy, chimePlayed);
}
