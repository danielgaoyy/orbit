import { useId } from "react";
import type { ProviderKind } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export function ProviderIcon({
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
      const m0 = `agy_m_${uid}`;
      const f1 = `agy_f1_${uid}`;
      const f2 = `agy_f2_${uid}`;
      const f3 = `agy_f3_${uid}`;
      const f5 = `agy_f5_${uid}`;
      const f6 = `agy_f6_${uid}`;
      const f7 = `agy_f7_${uid}`;
      const f8 = `agy_f8_${uid}`;
      const f9 = `agy_f9_${uid}`;
      const f10 = `agy_f10_${uid}`;
      const f11 = `agy_f11_${uid}`;

      return (
        <svg
          viewBox="0 0 24 24"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
          className={cn("size-3.5 shrink-0", className)}
        >
          <mask id={m0} maskUnits="userSpaceOnUse" x="0" y="1" width="24" height="23">
            <path
              fill="#fff"
              d="M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"
            />
          </mask>
          <g mask={`url(#${m0})`}>
            <g filter={`url(#${f1})`}>
              <path
                fill="#FFE432"
                d="M-1.018-3.992c-.408 3.591 2.686 6.89 6.91 7.37 4.225.48 7.98-2.043 8.387-5.633.408-3.59-2.686-6.89-6.91-7.37-4.225-.479-7.98 2.043-8.387 5.633z"
              />
            </g>
            <g filter={`url(#${f2})`}>
              <path
                fill="#FC413D"
                d="M15.269 7.747c1.058 4.557 5.691 7.374 10.348 6.293 4.657-1.082 7.575-5.653 6.516-10.21-1.058-4.556-5.691-7.374-10.348-6.292-4.657 1.082-7.575 5.653-6.516 10.21z"
              />
            </g>
            <g filter={`url(#${f3})`}>
              <path
                fill="#00B95C"
                d="M-12.443 10.804c1.338 4.703 7.36 7.11 13.453 5.378 6.092-1.733 9.947-6.95 8.61-11.652C8.282-.173 2.26-2.58-3.833-.848-9.925.884-13.78 6.1-12.443 10.804z"
              />
            </g>
            <g filter={`url(#${f5})`}>
              <path
                fill="#00B95C"
                d="M-7.608 14.703c3.352 3.424 9.126 3.208 12.896-.483 3.77-3.69 4.108-9.459.756-12.883C2.69-2.087-3.083-1.871-6.853 1.82c-3.77 3.69-4.108 9.458-.755 12.883z"
              />
            </g>
            <g filter={`url(#${f6})`}>
              <path
                fill="#3186FF"
                d="M9.932 27.617c1.04 4.482 5.384 7.303 9.7 6.3 4.316-1.002 6.971-5.448 5.93-9.93-1.04-4.483-5.384-7.304-9.7-6.301-4.316 1.002-6.971 5.448-5.93 9.93z"
              />
            </g>
            <g filter={`url(#${f7})`}>
              <path
                fill="#FBBC04"
                d="M2.572-8.185C.392-3.329 2.778 2.472 7.9 4.771c5.122 2.3 11.042.227 13.222-4.63 2.18-4.855-.205-10.656-5.327-12.955-5.122-2.3-11.042-.227-13.222 4.63z"
              />
            </g>
            <g filter={`url(#${f8})`}>
              <path
                fill="#3186FF"
                d="M-3.267 38.686c-5.277-2.072 3.742-19.117 5.984-24.83 2.243-5.712 8.34-8.664 13.616-6.592 5.278 2.071 11.533 13.482 9.29 19.195-2.242 5.713-23.613 14.298-28.89 12.227z"
              />
            </g>
            <g filter={`url(#${f9})`}>
              <path
                fill="#749BFF"
                d="M28.71 17.471c-1.413 1.649-5.1.808-8.236-1.878-3.135-2.687-4.531-6.201-3.118-7.85 1.412-1.649 5.1-.808 8.235 1.878s4.532 6.2 3.119 7.85z"
              />
            </g>
            <g filter={`url(#${f10})`}>
              <path
                fill="#FC413D"
                d="M18.163 9.077c5.81 3.93 12.502 4.19 14.946.577 2.443-3.612-.287-9.727-6.098-13.658-5.81-3.931-12.502-4.19-14.946-.577-2.443 3.612.287 9.727 6.098 13.658z"
              />
            </g>
            <g filter={`url(#${f11})`}>
              <path
                fill="#FFEE48"
                d="M-.915 2.684c-1.44 3.473-.97 6.967 1.05 7.804 2.02.837 4.824-1.3 6.264-4.772 1.44-3.473.97-6.967-1.05-7.804-2.02-.837-4.824 1.3-6.264 4.772z"
              />
            </g>
          </g>
          <defs>
            <filter id={f1} x="-3.288" y="-11.917" width="19.838" height="17.587" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="1.117" />
            </filter>
            <filter id={f2} x="4.251" y="-13.493" width="38.9" height="38.565" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="5.4" />
            </filter>
            <filter id={f3} x="-21.889" y="-10.592" width="40.955" height="36.517" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="4.591" />
            </filter>
            <filter id={f5} x="-19.099" y="-10.278" width="36.632" height="36.595" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="4.591" />
            </filter>
            <filter id={f6} x=".981" y="8.758" width="33.533" height="34.087" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="4.363" />
            </filter>
            <filter id={f7} x="-6.143" y="-21.659" width="35.978" height="35.276" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="3.954" />
            </filter>
            <filter id={f8} x="-11.96" y="-.46" width="45.114" height="46.523" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="3.531" />
            </filter>
            <filter id={f9} x="10.485" y=".58" width="25.094" height="24.054" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="3.159" />
            </filter>
            <filter id={f10} x="5.833" y="-12.467" width="33.508" height="30.007" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="2.669" />
            </filter>
            <filter id={f11} x="-8.355" y="-8.876" width="22.194" height="26.151" filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation="3.303" />
            </filter>
          </defs>
        </svg>
      );
    }
  }
}
