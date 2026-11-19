import { memo, useId } from "react";
import type { ProviderKind } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export const ProviderIcon = memo(function ProviderIcon({
  provider,
  className,
}: {
  provider: ProviderKind;
  className?: string;
}) {
  const uid = useId().replace(/:/g, "");

  switch (provider) {
    case "codex": {
      const gradId = `codex_grad_${uid}`;
      return (
        <svg
          viewBox="0 0 24 24"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
          className={cn("size-3.5 shrink-0", className)}
        >
          <path
            fill={`url(#${gradId})`}
            fillRule="evenodd"
            clipRule="evenodd"
            d="M8.086.457a6.105 6.105 0 0 1 3.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 0 0 .107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 0 1-.18 1.631.167.167 0 0 0 .04.155 5.982 5.982 0 0 1 1.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 0 1-2.934 1.851.162.162 0 0 0-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 0 0-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 0 1-2.595-.622 6.058 6.058 0 0 1-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 0 1-.495-1.283 6.11 6.11 0 0 1-.017-3.064.166.166 0 0 0 .008-.074.115.115 0 0 0-.037-.064 5.958 5.958 0 0 1-1.38-2.202 5.196 5.196 0 0 1-.333-1.589 6.915 6.915 0 0 1 .188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 0 0 .087-.087A6.016 6.016 0 0 1 5.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 0 0-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 0 0 1.46.864l1.94-3.272a.849.849 0 0 0 .007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 0 0 0 1.695h4.848a.849.849 0 0 0 0-1.696h-4.848z"
          />
          <defs>
            <linearGradient id={gradId} x1="12" y1="0" x2="12" y2="24" gradientUnits="userSpaceOnUse">
              <stop stopColor="#9E94FF" />
              <stop offset="0.5" stopColor="#6B8CFF" />
              <stop offset="1" stopColor="#3B43FF" />
            </linearGradient>
          </defs>
        </svg>
      );
    }

    case "claude_code":
      return (
        <svg
          viewBox="0 0 24 24"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
          className={cn("size-3.5 shrink-0", className)}
        >
          <path
            fill="#D97757"
            fillRule="nonzero"
            d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 0 1-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"
          />
        </svg>
      );

    case "antigravity": {
      // Pure GPU vector `<clipPath>` + `<radialGradient>` mesh (zero `<feGaussianBlur>` CPU filter passes)
      const clipId = `agy_clip_${uid}`;
      const gGreen = `agy_green_${uid}`;
      const gYellow = `agy_yellow_${uid}`;
      const gRed = `agy_red_${uid}`;

      return (
        <svg
          viewBox="0 0 24 24"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
          className={cn("size-3.5 shrink-0", className)}
        >
          <g clipPath={`url(#${clipId})`}>
            <rect x="0" y="0" width="24" height="24" fill="#3186FF" />
            <rect x="0" y="0" width="24" height="24" fill={`url(#${gGreen})`} />
            <rect x="0" y="0" width="24" height="24" fill={`url(#${gRed})`} />
            <rect x="0" y="0" width="24" height="24" fill={`url(#${gYellow})`} />
          </g>
          <defs>
            <clipPath id={clipId}>
              <path d="M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z" />
            </clipPath>
            <radialGradient id={gGreen} cx="2" cy="11" r="14" gradientUnits="userSpaceOnUse">
              <stop offset="0%" stopColor="#00B95C" />
              <stop offset="58%" stopColor="#00B95C" stopOpacity="0.82" />
              <stop offset="100%" stopColor="#00B95C" stopOpacity="0" />
            </radialGradient>
            <radialGradient id={gRed} cx="20" cy="6" r="13" gradientUnits="userSpaceOnUse">
              <stop offset="0%" stopColor="#FC413D" />
              <stop offset="55%" stopColor="#FC413D" stopOpacity="0.78" />
              <stop offset="100%" stopColor="#FC413D" stopOpacity="0" />
            </radialGradient>
            <radialGradient id={gYellow} cx="10.5" cy="2" r="10.5" gradientUnits="userSpaceOnUse">
              <stop offset="0%" stopColor="#FFE432" />
              <stop offset="55%" stopColor="#FBBC04" stopOpacity="0.9" />
              <stop offset="100%" stopColor="#FBBC04" stopOpacity="0" />
            </radialGradient>
          </defs>
        </svg>
      );
    }
  }
});
