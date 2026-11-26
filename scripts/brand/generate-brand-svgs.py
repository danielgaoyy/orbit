import math, pathlib, sys
OUT = pathlib.Path(sys.argv[1])
TILT = -28

def ell(cx, cy, rx, ry, deg):
    t = math.radians(TILT); a = math.radians(deg)
    x, y = rx*math.cos(a), ry*math.sin(a)
    return cx + x*math.cos(t) - y*math.sin(t), cy + x*math.sin(t) + y*math.cos(t)

def n(v): return f"{v:.2f}".rstrip("0").rstrip(".")

def halves(cx, cy, rx, ry):
    p0 = ell(cx, cy, rx, ry, 0); p1 = ell(cx, cy, rx, ry, 180)
    front = f"M{n(p0[0])} {n(p0[1])}A{n(rx)} {n(ry)} {TILT} 0 1 {n(p1[0])} {n(p1[1])}"
    back = f"M{n(p1[0])} {n(p1[1])}A{n(rx)} {n(ry)} {TILT} 0 1 {n(p0[0])} {n(p0[1])}"
    return front, back

# ---- 1024 artwork (shared by macOS + full-bleed) ----
C, RX, RY, R, RW, OW = 512, 372, 98, 205, 80, 12
FRONT, BACK = halves(C, C, RX, RY)
SX, SY = ell(C, C, RX, RY, 352)

def art():
    return f'''  <path d="{BACK}" stroke="#F1F2F7" stroke-opacity=".35" stroke-width="{OW}" stroke-linecap="round"/>
  <circle cx="512" cy="530" r="{R}" stroke="#000" stroke-opacity=".35" stroke-width="{RW}" filter="url(#orbit-soft)"/>
  <circle cx="512" cy="512" r="{R}" stroke="url(#orbit-ring)" stroke-width="{RW}"/>
  <path d="{FRONT}" stroke="#1B1C23" stroke-width="40" clip-path="url(#orbit-ring-area)"/>
  <path d="{FRONT}" stroke="#F1F2F7" stroke-width="{OW}" stroke-linecap="round"/>
  <circle cx="{n(SX)}" cy="{n(SY)}" r="46" fill="#3ED598" fill-opacity=".5" filter="url(#orbit-soft)"/>
  <circle cx="{n(SX)}" cy="{n(SY)}" r="30" fill="#3ED598"/>'''

DEFS = '''    <linearGradient id="orbit-bg" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#272A35"/><stop offset="1" stop-color="#121318"/>
    </linearGradient>
    <linearGradient id="orbit-ring" x1="320" y1="300" x2="700" y2="730" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#8B91A5"/>
    </linearGradient>
    <clipPath id="orbit-ring-area"><circle cx="512" cy="512" r="246"/></clipPath>
    <filter id="orbit-soft" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="24"/></filter>'''

mac = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" fill="none">
  <!-- Orbit app icon (macOS / Windows / Linux): 824px squircle on a 1024 canvas per Apple's icon grid. -->
  <defs>
{DEFS}
    <linearGradient id="orbit-rim" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity=".22"/><stop offset=".5" stop-color="#FFFFFF" stop-opacity=".04"/><stop offset="1" stop-color="#FFFFFF" stop-opacity=".1"/>
    </linearGradient>
    <filter id="orbit-drop" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#000" flood-opacity=".28"/>
    </filter>
    <clipPath id="orbit-squircle"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#orbit-bg)" filter="url(#orbit-drop)"/>
  <g clip-path="url(#orbit-squircle)">
{art()}
  </g>
  <rect x="101.5" y="101.5" width="821" height="821" rx="183.5" stroke="url(#orbit-rim)" stroke-width="3"/>
</svg>
'''

# Full-bleed opaque square for iOS / Android / HarmonyOS (the OS applies its own mask).
S = 1024/824
full = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" fill="none">
  <!-- Orbit app icon, full-bleed opaque square for platforms that apply their own mask (iOS, Android, HarmonyOS). -->
  <defs>
{DEFS}
  </defs>
  <g transform="translate(512 512) scale({S:.5f}) translate(-512 -512)">
    <rect x="0" y="0" width="1024" height="1024" fill="url(#orbit-bg)"/>
{art()}
  </g>
</svg>
'''

# ---- Flat mark: 32 grid, transparent, ring knocked out where the front orbit crosses ----
def flat_mark(ink, sat, back_op):
    c, rx, ry, r, rw, ow = 16, 13.6, 3.6, 7.7, 3.2, 1.4
    f, b = halves(c, c, rx, ry)
    sx, sy = ell(c, c, rx, ry, 352)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none">
  <defs>
    <mask id="orbit-mark-cut" maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="32">
      <rect width="32" height="32" fill="#fff"/>
      <path d="{f}" stroke="#000" stroke-width="{n(ow+2.2)}"/>
    </mask>
  </defs>
  <path d="{b}" stroke="{ink}" stroke-opacity="{back_op}" stroke-width="{ow}" stroke-linecap="round"/>
  <circle cx="16" cy="16" r="{r}" stroke="{ink}" stroke-width="{rw}" mask="url(#orbit-mark-cut)"/>
  <path d="{f}" stroke="{ink}" stroke-width="{ow}" stroke-linecap="round"/>
  <circle cx="{n(sx)}" cy="{n(sy)}" r="2.2" fill="{sat}"/>
</svg>
'''

# ---- macOS menu bar template: pure black + alpha, 36px (= 18pt @2x) ----
def tray():
    c, rx, ry, r, rw, ow = 18, 15.0, 4.15, 8.6, 3.5, 2.0
    f, b = halves(c, c, rx, ry)
    sx, sy = ell(c, c, rx, ry, 352)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="53" height="36" viewBox="2.1 6.15 32.94 22.37" fill="none">
  <!-- macOS menu bar template image: black + alpha only; AppKit tints it for light/dark menu bars.
       Cropped to the mark so the 18pt-tall status item shows it at ~16pt like system glyphs. -->
  <defs>
    <mask id="orbit-tray-cut" maskUnits="userSpaceOnUse" x="0" y="0" width="36" height="36">
      <rect x="0" y="0" width="38" height="36" fill="#fff"/>
      <path d="{f}" stroke="#000" stroke-width="{n(ow+2.6)}"/>
      <circle cx="{n(sx)}" cy="{n(sy)}" r="4.2" fill="#000"/>
    </mask>
  </defs>
  <path d="{b}" stroke="#000" stroke-opacity=".55" stroke-width="{ow}" stroke-linecap="round" mask="url(#orbit-tray-sat)"/>
  <circle cx="18" cy="18" r="{r}" stroke="#000" stroke-width="{rw}" mask="url(#orbit-tray-cut)"/>
  <path d="{f}" stroke="#000" stroke-width="{ow}" stroke-linecap="round"/>
  <circle cx="{n(sx)}" cy="{n(sy)}" r="2.7" fill="#000"/>
</svg>
'''.replace(' mask="url(#orbit-tray-sat)"', '')

# ---- Favicon: compact app tile so it stays legible on light and dark tab bars ----
def favicon():
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32" fill="none">
  <!-- Orbit favicon: the app tile reduced to 32px (same geometry as orbit-app-icon.svg). -->
  <defs>
{DEFS}
    <clipPath id="orbit-favicon-clip"><rect width="32" height="32" rx="7"/></clipPath>
  </defs>
  <g clip-path="url(#orbit-favicon-clip)">
    <rect width="32" height="32" fill="url(#orbit-bg)"/>
    <g transform="translate(16 16) scale(0.03883) translate(-512 -512)">
{art()}
    </g>
  </g>
</svg>
'''

files = {
  "orbit-favicon.svg": favicon(),
  "orbit-app-icon.svg": mac,
  "orbit-app-icon-fullbleed.svg": full,
  "orbit-mark.svg": flat_mark("currentColor", "#3ED598", ".45"),
  "orbit-mark-dark.svg": flat_mark("#F1F2F7", "#3ED598", ".45"),
  "orbit-mark-light.svg": flat_mark("#14161D", "#0F7B55", ".4"),
  "orbit-tray-template.svg": tray(),
}
for k, v in files.items(): (OUT / k).write_text(v)
print(f"front={FRONT}\nback={BACK}\nsat={SX:.2f},{SY:.2f}")
