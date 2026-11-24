/** All geometry uses a 64-unit canvas. Paired paths describe the animal's left side. */
export interface AvatarPath {
  readonly fill: string;
  readonly d: string;
}

type Layer = readonly AvatarPath[];

interface Portrait {
  readonly back: Layer;
  readonly head: Layer;
  readonly face: Layer;
  readonly eyes: Layer;
}

export interface SubagentCharacterSpec extends Portrait {
  readonly id: string;
  readonly name: string;
  readonly lightBg: string;
  readonly darkBg: string;
}

const ink = "#302934";
const ivory = "#FFF1D9";
const milk = "#FFF9ED";
const rust = "#D16B39";
const rose = "#DEA29E";

function paint(fill: string, d: string): AvatarPath {
  return { fill, d };
}

type Point = readonly [x: number, y: number];
type Segment =
  | { readonly kind: "L"; readonly start: Point; readonly end: Point }
  | { readonly kind: "Q"; readonly start: Point; readonly c: Point; readonly end: Point }
  | { readonly kind: "C"; readonly start: Point; readonly c1: Point; readonly c2: Point; readonly end: Point };

function formatCoord(n: number): string {
  return Number.isInteger(n) ? String(n) : String(Number(n.toFixed(3)));
}

function mirrorPoint([x, y]: Point): string {
  return `${formatCoord(64 - x)} ${formatCoord(y)}`;
}

/**
 * When a single subpath starts and ends on the centerline (`x = 32`), join its mirrored half
 * in reverse so the shape becomes ONE continuous closed contour with zero interior seam at
 * `x = 32` (eliminating anti-aliasing subpixel cracks during CSS transform animations).
 */
function fuseCenterlineSymmetric(d: string): string | null {
  const trimmed = d.trim();
  const body = trimmed.endsWith("Z") ? trimmed.slice(0, -1) : trimmed;
  if (body.includes("Z") || body.indexOf("M", 1) !== -1) return null;

  const segments: Segment[] = [];
  let start: Point | null = null;
  let cur: Point = [0, 0];
  let prevCubicCtrl: Point | null = null;
  let prevQuadCtrl: Point | null = null;

  for (const [, cmd, rawArgs] of body.matchAll(/([A-Z])([^A-Z]*)/g)) {
    const nums = rawArgs.trim() ? rawArgs.trim().split(/\s+/).map(Number) : [];
    switch (cmd) {
      case "M":
        cur = [nums[0]!, nums[1]!];
        start = cur;
        prevCubicCtrl = null;
        prevQuadCtrl = null;
        break;
      case "L": {
        const end: Point = [nums[0]!, nums[1]!];
        segments.push({ kind: "L", start: cur, end });
        cur = end;
        prevCubicCtrl = null;
        prevQuadCtrl = null;
        break;
      }
      case "H": {
        const end: Point = [nums[0]!, cur[1]];
        segments.push({ kind: "L", start: cur, end });
        cur = end;
        prevCubicCtrl = null;
        prevQuadCtrl = null;
        break;
      }
      case "V": {
        const end: Point = [cur[0], nums[0]!];
        segments.push({ kind: "L", start: cur, end });
        cur = end;
        prevCubicCtrl = null;
        prevQuadCtrl = null;
        break;
      }
      case "Q": {
        const c: Point = [nums[0]!, nums[1]!];
        const end: Point = [nums[2]!, nums[3]!];
        segments.push({ kind: "Q", start: cur, c, end });
        cur = end;
        prevQuadCtrl = c;
        prevCubicCtrl = null;
        break;
      }
      case "T": {
        const c: Point = prevQuadCtrl
          ? [2 * cur[0] - prevQuadCtrl[0], 2 * cur[1] - prevQuadCtrl[1]]
          : cur;
        const end: Point = [nums[0]!, nums[1]!];
        segments.push({ kind: "Q", start: cur, c, end });
        cur = end;
        prevQuadCtrl = c;
        prevCubicCtrl = null;
        break;
      }
      case "C": {
        const c1: Point = [nums[0]!, nums[1]!];
        const c2: Point = [nums[2]!, nums[3]!];
        const end: Point = [nums[4]!, nums[5]!];
        segments.push({ kind: "C", start: cur, c1, c2, end });
        cur = end;
        prevCubicCtrl = c2;
        prevQuadCtrl = null;
        break;
      }
      case "S": {
        const c1: Point = prevCubicCtrl
          ? [2 * cur[0] - prevCubicCtrl[0], 2 * cur[1] - prevCubicCtrl[1]]
          : cur;
        const c2: Point = [nums[0]!, nums[1]!];
        const end: Point = [nums[2]!, nums[3]!];
        segments.push({ kind: "C", start: cur, c1, c2, end });
        cur = end;
        prevCubicCtrl = c2;
        prevQuadCtrl = null;
        break;
      }
      default:
        return null;
    }
  }

  if (!start || segments.length === 0 || start[0] !== 32 || cur[0] !== 32) {
    return null;
  }

  let mirroredBack = "";
  for (let i = segments.length - 1; i >= 0; i -= 1) {
    const seg = segments[i]!;
    if (seg.kind === "L") {
      mirroredBack += `L${mirrorPoint(seg.start)}`;
    } else if (seg.kind === "Q") {
      mirroredBack += `Q${mirrorPoint(seg.c)} ${mirrorPoint(seg.start)}`;
    } else {
      mirroredBack += `C${mirrorPoint(seg.c2)} ${mirrorPoint(seg.c1)} ${mirrorPoint(seg.start)}`;
    }
  }
  return `${body}${mirroredBack}Z`;
}

/** Reflect absolute (x, y) pairs about the centerline. Centerline-spanning shapes are fused
 * into a single continuous closed path so animations never reveal an anti-aliased seam at x=32. */
function mirrored(d: string): string {
  const fused = fuseCenterlineSymmetric(d);
  if (fused) return fused;
  return d + d.replace(/(-?[\d.]+) (-?[\d.]+)/g, (_, x, y) => `${64 - Number(x)} ${y}`);
}

function pair(fill: string, d: string): AvatarPath {
  return paint(fill, mirrored(d));
}

/** A half profile starts and ends on x=32; fused into one continuous closed contour. */
function symmetric(profile: string): string {
  return mirrored(profile);
}

function oval(x: number, y: number, rx: number, ry: number): string {
  const left = x - rx, right = x + rx, top = y - ry, bottom = y + ry;
  return `M${left} ${y}Q${left} ${top} ${x} ${top}T${right} ${y}T${x} ${bottom}T${left} ${y}Z`;
}

type Ear = readonly [outer: string, inner: string];

function roundEar(x: number, y: number, rx: number, ry: number): Ear {
  return [oval(x, y, rx, ry), oval(x, y, rx - 3, ry - 3)];
}

function ears([outer, inner]: Ear, fur: string, lining: string): Layer {
  return [pair(fur, outer), pair(lining, inner)];
}

// The ear tip, spread and bend distinguish the fox's point from the corgi's cup.
function uprightEar(x: number, y: number, spread: number, bend: number): Ear {
  return [
    `M${x - 2} 32Q${x - bend} 18 ${x} ${y}Q${x + 2} ${y - bend} ${x + 5} ${y + 2}L${x + spread} 22Z`,
    `M${x + 2} 24L${x + 2} ${y + 3}L${x + spread - 5} 22Z`,
  ];
}

const pointed = uprightEar(14, 11, 16, 3);
const kitten = uprightEar(12, 13, 17, 1);
const lopEar: Ear = ["M23 19C10 10 5 27 5 41S16 54 19 40L25 24Z", "M17 24Q9 29 10 44Q13 49 15 39Z"];
const leaf: Ear = ["M24 29Q13 28 9 20Q8 18 12 17Q20 17 26 23Z", "M21 24Q15 24 12 19Q18 19 21 24Z"];
const tuftEar: Ear = ["M11 30Q12 20 14 11L18 14L18 8Q24 13 25 24Z", "M16 23L16 15Q21 16 22 23Z"];
const buttonEar = roundEar(15, 23, 8, 8);
const owlTufts = symmetric("M32 18Q26 18 22 21L12 17Q11 23 11 31L32 33");

const round = symmetric("M32 15C17 15 9 26 9 39S21 55 32 55");
const ruff = symmetric("M32 15Q18 15 14 29L7 35L12 37L9 41Q21 44 28 51Q30 54 32 54");
const tapered = symmetric("M32 15Q15 15 12 32Q9 44 24 52Q29 55 32 55");
const dome = symmetric("M32 10C18 10 13 23 11 35Q5 54 32 55");
const long = symmetric("M32 16Q14 15 14 34L16 46Q18 57 32 57");

function coat(shape: string, fur: string, ...markings: AvatarPath[]): Layer {
  // The sweep follows the forehead, with a higher hairline on the domed birds.
  const light = shape === dome ? "M15 28Q18 12 33 12Q23 15 15 28Z" : "M15 30Q18 18 32 17Q23 20 15 30Z";
  return [paint(fur, shape), paint("#FFFFFF45", light), ...markings];
}

const cheeks = pair(ivory, "M32 44Q22 34 9 35L14 39L11 40Q22 43 29 51Q30 53 32 53Z");
const bib = pair(ivory, "M32 37Q28 37 25 40Q19 36 10 38Q12 53 32 54Z");
const brows = pair(ivory, "M16 29Q18 22 24 25Q27 29 21 30Q17 32 16 29Z");
const pads = pair(ivory, "M32 39Q24 33 19 40Q15 48 25 49Q29 50 32 47Z");
const whiskers = pair("#645059", "M9 40Q14 39 20 42L19 43L9 42ZM12 47L20 44L20 46L12 49Z");
const smile = pair("#755B58", "M32 44Q31 48 26 47Q28 51 32 48Z");
const nose = pair(ink, "M32 40Q25 39 28 43Q30 46 32 46Z");
const snout = [smile, nose];
const whiskered = [whiskers, ...snout];
const beak = [paint("#B7733F", "M27 38Q32 34 37 38Q36 43 32 47Q28 43 27 38Z")];
const antlers = pair("#815538", "M24 24L22 17Q18 16 17 12Q17 9 19 11L22 14L21 9Q22 6 24 9L25 15L27 12Q30 11 29 14L26 18L28 22Z");

function eyes(x = 21, y = 35, radius = 3.3, iris?: string): Layer {
  function both(offset: number, height: number, r: number) {
    return oval(x + offset, height, r, r) + oval(64 - x + offset, height, r, r);
  }
  return [
    ...(iris ? [paint(iris, both(0, y, radius + 2))] : []),
    paint(ink, both(0, y, radius)),
    paint("#FFFFFF", both(-1, y - 1.2, 1.25)),
  ];
}

const brightEyes = eyes();

type Backdrop = readonly [light: string, dark: string];
const clay: Backdrop = ["#F5ECE3", "#2B2522"];
const oat: Backdrop = ["#F3EDE2", "#2B2721"];
const sage: Backdrop = ["#E7EFE9", "#222A26"];
const tide: Backdrop = ["#E5EFF2", "#21292E"];
const frost: Backdrop = ["#E4EDF3", "#222830"];
const pebble: Backdrop = ["#EDEBF2", "#26272D"];
const blush: Backdrop = ["#F4E9E7", "#2C2425"];

function animal(
  id: string, [lightBg, darkBg]: Backdrop,
  back: Layer, head: Layer, face: Layer = snout, gaze: Layer = brightEyes,
  name = id.split("-").map(word => word[0].toUpperCase() + word.slice(1)).join(" "),
): SubagentCharacterSpec {
  return { id, name, lightBg, darkBg, back, head, face, eyes: gaze };
}

export const SUBAGENT_AVATAR_ROSTER: readonly SubagentCharacterSpec[] = [
  animal("red-fox", clay,
    ears(pointed, "#633E34", "#E49B64"), coat(ruff, rust, cheeks)),
  animal("shiba-inu", oat,
    ears(pointed, "#BD804F", "#E6AA87"), coat(round, "#D79B5F", bib, brows)),
  animal("orange-tabby", clay,
    ears(kitten, rust, rose), coat(round, "#E9A45F", pads,
      paint("#9C5533", symmetric("M32 22L28 25L26 20L22 18L25 29L32 26"))),
    whiskered),
  animal("giant-panda", sage,
    ears(roundEar(18, 20, 9, 9), ink, "#514650"),
    coat(round, "#E0DED4", pair("#605863", "M15 33Q18 25 25 28Q30 31 24 39Q17 47 13 40Q12 37 15 33Z"))),
  animal("sea-otter", tide,
    ears(roundEar(11, 30, 6, 6), "#725044", "#B7856D"),
    coat(round, "#AC775A", pads), whiskered),
  animal("barn-owl", oat,
    [paint("#AB7B52", owlTufts)], coat(round, "#BB9268",
      pair(milk, "M32 27C18 13 6 28 14 41Q21 50 32 54Z")),
    beak, eyes(21, 34, 4.3)),
  animal("emperor-penguin", frost,
    [paint("#E8AB54", oval(32, 40, 24, 15))], coat(dome, "#384959",
      pair(milk, "M32 39Q25 17 16 31Q7 40 14 47Q19 54 32 54Z")),
    [pair("#DF884A", "M32 38Q27 37 25 40L32 45Z")]),
  animal("capybara", oat,
    ears(roundEar(20, 16, 6, 7), "#916544", "#65483F"),
    coat(long, "#BC936C", pair("#A17856", "M32 34Q21 33 19 40L19 47Q20 53 32 54Z")),
    [pair("#5D453A", "M24 41Q24 37 29 39L28 42Z"), paint("#695042", "M26 48Q32 50 38 48Q33 53 26 49Z")],
    eyes(20, 31, 2.9)),
  animal("red-panda", blush,
    ears(roundEar(15, 22, 8, 9), ivory, "#794941"),
    coat(ruff, "#BE613E", brows, pair(ivory, "M10 37Q15 34 21 38L17 40L23 46Q15 43 10 40Z"), pads)),
  animal("holland-lop", blush,
    ears(lopEar, "#A4776E", rose), coat(tapered, "#D1A99C", pads),
    [smile, paint("#946765", "M28 41Q32 39 36 41L32 46Z")]),
  animal("corgi", oat,
    ears(uprightEar(15, 12, 15, 7), "#BE804E", rose), coat(tapered, "#E3A161", bib,
      pair(milk, "M32 15L29 16L30 27L26 41L32 43Z"))),
  animal("sika-deer", sage,
    [...ears(leaf, "#B77D55", "#E6B397"), antlers], coat(tapered, "#C68D61",
      pair(ivory, oval(23, 26, 2.8, 2) + oval(17, 29, 2, 1.6)), pads)),
  animal("koala", sage,
    ears(roundEar(15, 25, 10, 10), "#87989C", "#D5BFC0"), coat(symmetric("M32 16Q17 16 12 29L8 34L12 35L8 39L13 40Q12 53 32 54"), "#ABB7B9"),
    [pair(ink, "M32 32Q27 31 26 40Q24 46 32 46Z"), smile]),
  animal("raccoon", pebble,
    ears(buttonEar, "#75818A", "#464551"), coat(ruff, "#ACB1B1", cheeks,
      pair("#504D59", "M32 33L26 29Q18 25 9 33Q15 42 23 41L32 36Z"))),
  animal("arctic-fox", frost,
    ears(pointed, "#BBCAD1", "#899CAB"), coat(ruff, "#C6D4DC", cheeks, brows)),
  animal("russian-blue", pebble,
    ears(kitten, "#758799", "#AA98A8"), coat(tapered, "#94A4B8", pair("#C2CAD1", oval(27, 43, 6, 4))),
    whiskered, eyes(21, 34, 2.9, "#BCD2AB")),
  animal("husky", frost,
    ears(pointed, "#546474", "#CCD2D3"), coat(tapered, "#748493",
      pair(milk, "M32 27L29 34L22 27Q15 22 12 36Q13 47 30 54L32 54Z"))),
  animal("cream-alpaca", oat,
    [...ears(uprightEar(18, 9, 8, 3), "#BCA58B", rose),
      pair(milk, "M32 12Q25 8 22 14Q14 12 14 21Q11 28 19 31L32 32Z")],
    coat(symmetric("M32 25Q26 29 23 24Q17 28 15 23Q10 56 32 57"), "#DAC3A4", pads)),
  animal("european-hedgehog", oat,
    [pair("#795545", "M32 8Q30 10 27 15L22 10L20 19L14 16L13 25L8 24L10 33L6 33Q6 55 32 56Z")],
    coat(symmetric("M32 28Q21 14 14 28Q5 46 28 54Q30 56 32 56"), "#DEB990"),
    snout, eyes(22, 36, 3), "Hedgehog"),
  animal("red-squirrel", clay,
    [paint("#AE553A", "M22 52C4 51 1 32 9 24Q14 19 19 23Q10 27 17 32Q26 37 22 52Z"), ...ears(tuftEar, "#A85338", "#E3A175")],
    coat(tapered, rust, pads),
    [paint(milk, "M29 46H35V51Q32 54 29 51Z"), ...snout]),
  animal("harbor-seal", tide,
    [paint("#849CA7", oval(32, 41, 24, 14))],
    coat(round, "#B1C4CA", pair("#849CA7", oval(24, 22, 3, 2) + oval(18, 26, 2, 1.6)), pads),
    [whiskers, smile, pair(ink, "M27 40Q31 39 32 45Q27 44 27 40Z")]),
  animal("snowy-owl", frost,
    [paint("#B3C1C9", owlTufts)], coat(round, "#D2DCE0",
      pair(milk, oval(21, 34, 10, 11)), pair("#84939F", "M17 22L23 23L27 21Q24 28 17 24Z")),
    beak, eyes(21, 34, 3.6, "#E5B956")),
  animal("mallard-duck", sage,
    [paint("#EEE5CD", oval(32, 46, 20, 12))], coat(dome, "#357C6C",
      paint("#559680", "M13 31Q15 19 24 17Q19 24 23 29Z")),
    [pair("#F0B84F", "M32 37Q29 37 26 40Q17 39 17 44Q17 50 32 50Z"),
      pair("#946831", oval(27, 42, 1.8, 1.3))]),
  animal("gray-chinchilla", pebble,
    ears(roundEar(19, 21, 10, 11), "#8D899F", "#C9AEBA"), coat(tapered, "#A8A5BA", pads),
    whiskered, eyes(21, 35, 3.7), "Chinchilla"),
];

/** FNV-1a 32-bit stable hash for deterministic avatar slot assignment */
export function hashSubagentAvatarSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * Resolves a stable, deterministic character for any standalone identity key
 * (e.g. a timeline card or detached Subagent tab without full sibling list).
 */
export function resolveSubagentCharacter(seedKey: string): SubagentCharacterSpec {
  const normalized = seedKey.trim() || "subagent";
  const index = hashSubagentAvatarSeed(normalized) % SUBAGENT_AVATAR_ROSTER.length;
  return SUBAGENT_AVATAR_ROSTER[index]!;
}

export function getSubagentCharacterById(id: string | null | undefined): SubagentCharacterSpec | null {
  return SUBAGENT_AVATAR_ROSTER.find(character => character.id === id) ?? null;
}

/**
 * Assigns collision-free characters to a session's subagent list:
 * - Deterministic per `stableSeed` across re-renders & status updates
 * - Sorts keys canonically before linear-probing so changing status order (e.g. running -> completed)
 *   NEVER swaps two subagents' faces mid-session
 * - Guarantees zero duplicate characters for the first 24 concurrent subagents in a session
 */
export function assignSubagentCharacters<T extends { readonly key: string }>(
  items: readonly T[],
  getSeed?: (item: T) => string,
): Map<string, SubagentCharacterSpec> {
  const out = new Map<string, SubagentCharacterSpec>();

  const rosterSize = SUBAGENT_AVATAR_ROSTER.length;
  const canonical = items
    .map((item) => ({
      key: item.key,
      seed: (getSeed ? getSeed(item) : item.key).trim() || item.key,
    }))
    .sort((a, b) => a.seed.localeCompare(b.seed) || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));

  const usedIndices = new Set<number>();
  for (const entry of canonical) {
    let chosen = hashSubagentAvatarSeed(entry.seed) % rosterSize;
    if (usedIndices.size < rosterSize) {
      while (usedIndices.has(chosen)) chosen = (chosen + 1) % rosterSize;
      usedIndices.add(chosen);
    }
    out.set(entry.key, SUBAGENT_AVATAR_ROSTER[chosen]!);
  }

  return out;
}
