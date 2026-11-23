import { memo, useEffect, useMemo, useRef } from "react";
import { Avatar, type AvatarHandle } from "@oneworks/avatar-react/renderer";
import type { ResolvedSubagentStatus } from "@/lib/subagents";
import {
  SUBAGENT_AWAITING_CLIP,
  SUBAGENT_WORKING_CLIP,
  type SubagentCharacterSpec,
  buildSubagentAvatarDefinition,
} from "@/lib/subagent-avatar";

export interface LiveSubagentAvatarProps {
  spec: SubagentCharacterSpec;
  resolvedTheme: "light" | "dark";
  status: ResolvedSubagentStatus;
  shouldAnimate: boolean;
}

/**
 * Lazily loaded live OneWorks 3D `<Avatar>` mounted ONLY while a visible subagent
 * is actively running or awaiting approval.
 */
const LiveSubagentAvatar = memo(function LiveSubagentAvatar({
  spec,
  resolvedTheme,
  status,
  shouldAnimate,
}: LiveSubagentAvatarProps) {
  const handleRef = useRef<AvatarHandle | null>(null);

  const definition = useMemo(
    () => buildSubagentAvatarDefinition(spec, resolvedTheme),
    [spec, resolvedTheme],
  );

  const activeClip = !shouldAnimate
    ? null
    : status === "awaiting_permission"
      ? SUBAGENT_AWAITING_CLIP
      : SUBAGENT_WORKING_CLIP;

  useEffect(() => {
    if (!shouldAnimate) {
      handleRef.current?.stop({ reset: true });
    }
  }, [shouldAnimate, status]);

  return (
    <Avatar
      ref={handleRef}
      definition={definition}
      interactive={false}
      animation={activeClip}
      autoplay={shouldAnimate}
      theme={resolvedTheme}
      aria-hidden="true"
      className="block size-full"
    />
  );
});

export default LiveSubagentAvatar;
