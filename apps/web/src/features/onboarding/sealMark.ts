// Script-aware monogram derivation for the wax seal.
//
// Direct port of `tmp/NewCampaignOnboarding/wax_seal.jsx`. The behaviour is
// load-bearing: for "The Embergrove Saga" (Latin, with English stop-word
// "The") this returns "ES", the first grapheme of each non-stop-word, up
// to three. For "蒼穹のファフナー" it takes the first grapheme; for
// "العاصفة" it returns the whole first word as a calligraphic block; for
// empty / punctuation-only / symbol-only names it returns null so the
// caller falls back to the raven mark.

export type SealScript =
  | "Han"
  | "Hangul"
  | "Hiragana"
  | "Katakana"
  | "Arabic"
  | "Hebrew"
  | "Devanagari"
  | "Bengali"
  | "Tamil"
  | "Thai"
  | "Cyrillic"
  | "Greek"
  | "Latin";

export interface SealMark {
  text: string;
  script: SealScript;
  charCount: number;
}

const SCRIPT_TESTS: ReadonlyArray<[SealScript, RegExp]> = [
  ["Han", /\p{Script=Han}/u],
  ["Hangul", /\p{Script=Hangul}/u],
  ["Hiragana", /\p{Script=Hiragana}/u],
  ["Katakana", /\p{Script=Katakana}/u],
  ["Arabic", /\p{Script=Arabic}/u],
  ["Hebrew", /\p{Script=Hebrew}/u],
  ["Devanagari", /\p{Script=Devanagari}/u],
  ["Bengali", /\p{Script=Bengali}/u],
  ["Tamil", /\p{Script=Tamil}/u],
  ["Thai", /\p{Script=Thai}/u],
  ["Cyrillic", /\p{Script=Cyrillic}/u],
  ["Greek", /\p{Script=Greek}/u],
];

const EN_STOPS = new Set([
  "the",
  "a",
  "an",
  "of",
  "and",
  "in",
  "on",
  "at",
  "by",
  "for",
  "to",
  "&",
  "vs",
  "vs.",
]);

function primaryScript(s: string): SealScript {
  for (const ch of s) {
    if (!/\p{L}/u.test(ch)) continue;
    for (const [name, re] of SCRIPT_TESTS) {
      if (re.test(ch)) return name;
    }
    return "Latin";
  }
  return "Latin";
}

// Naive .slice() breaks combining marks ("น้ำ"), surrogate pairs, ZWJ, and
// Hangul jamo. Intl.Segmenter is the only correct way to take "the first
// visible character".
function firstGraphemes(s: string, n: number, locale?: string): string {
  if (typeof Intl !== "undefined" && Intl.Segmenter) {
    const seg = new Intl.Segmenter(locale, { granularity: "grapheme" });
    return [...seg.segment(s)]
      .slice(0, n)
      .map((g) => g.segment)
      .join("");
  }
  return Array.from(s).slice(0, n).join("");
}

function firstWord(s: string): string {
  return s.trim().split(/\s+/)[0] ?? "";
}

// Filter English stop-words only; for other locales we leave them in
// (better to render "LDR" for "Le Dernier Royaume" than guess wrong).
function alphabeticInitials(s: string, locale?: string): string {
  const isEnglish = !locale || /^en\b/i.test(locale);
  const words = s
    .trim()
    .split(/\s+/)
    .filter((w) => {
      if (!w) return false;
      if (isEnglish && EN_STOPS.has(w.toLowerCase())) return false;
      return /\p{L}/u.test(w);
    });
  const fallback = s.trim().split(/\s+/);
  const picked = (words.length > 0 ? words : fallback).slice(0, 3);
  const out = picked.map((w) => firstGraphemes(w, 1, locale)).join("");
  try {
    return out.toLocaleUpperCase(locale ?? undefined);
  } catch {
    return out.toUpperCase();
  }
}

/**
 * Returns the seal monogram for `rawName`, or null if no derivable mark
 * (in which case the seal renders the raven fallback). `locale` is BCP-47
 * (e.g. "en", "fr-CA"); used for stop-word filtering and casing.
 */
export function sealMark(rawName: string, locale?: string): SealMark | null {
  const name = (rawName || "").trim();
  if (!name) return null;
  // Strip leading/trailing punctuation/quotes so "«Имя»" reads as "Имя".
  const stripped = name.replace(/^[\p{P}\p{S}\s]+|[\p{P}\p{S}\s]+$/gu, "");
  if (!stripped) return null;
  if (!/\p{L}/u.test(stripped)) return null;

  const script = primaryScript(stripped);
  let text: string;
  switch (script) {
    case "Han":
    case "Hiragana":
    case "Katakana":
    case "Hangul":
    case "Devanagari":
    case "Bengali":
    case "Tamil":
    case "Thai":
      text = firstGraphemes(stripped, 1, locale);
      break;
    case "Arabic":
    case "Hebrew":
      // No initials tradition; use the whole first word as a calligraphic
      // block. Fit-to-circle handles the width.
      text = firstWord(stripped);
      break;
    case "Cyrillic":
    case "Greek":
    case "Latin":
    default:
      text = alphabeticInitials(stripped, locale);
      break;
  }

  if (!text) return null;
  return { text, script, charCount: [...text].length };
}

// Per-script font stacks. The page's display face (Cormorant Garamond) covers
// Latin / Cyrillic / Greek but has no CJK / Brahmic / Arabic glyphs.
export const FONT_BY_SCRIPT: Record<SealScript, string> = {
  Han: '"Songti SC", "STSong", "PingFang SC", "Noto Serif SC", serif',
  Hiragana: '"Hiragino Mincho ProN", "YuMincho", "Noto Serif JP", serif',
  Katakana: '"Hiragino Mincho ProN", "YuMincho", "Noto Serif JP", serif',
  Hangul: '"Apple SD Gothic Neo", "Nanum Myeongjo", "Noto Serif KR", serif',
  Arabic: '"Amiri", "Scheherazade New", "Noto Naskh Arabic", serif',
  Hebrew: '"Frank Ruhl Libre", "David", "Noto Serif Hebrew", serif',
  Devanagari: '"Noto Serif Devanagari", "Sanskrit Text", serif',
  Bengali: '"Noto Serif Bengali", serif',
  Tamil: '"Noto Serif Tamil", serif',
  Thai: '"Noto Serif Thai", "Sarabun", serif',
  Cyrillic: "var(--font-display, serif)",
  Greek: "var(--font-display, serif)",
  Latin: "var(--font-display, serif)",
};

// Per-script visual nudges. CJK chars sit on a square em and don't want
// letter-spacing; Latin initials look tighter when slightly negative.
export const TRACKING_BY_SCRIPT: Record<SealScript, string> = {
  Han: "0",
  Hiragana: "0",
  Katakana: "0",
  Hangul: "0",
  Arabic: "0",
  Hebrew: "0",
  Devanagari: "0",
  Bengali: "0",
  Tamil: "0",
  Thai: "0",
  Latin: "-0.04em",
  Cyrillic: "-0.02em",
  Greek: "-0.02em",
};

const SQUARE_SCRIPTS: ReadonlySet<SealScript> = new Set(["Han", "Hiragana", "Katakana", "Hangul"]);

/**
 * Pick the SVG textLength + fontSize for a mark inside the 100x100 viewBox.
 * Mirrors the wireframe's rules:
 * - Single char in CJK: smaller textLength (56) + bigger size (70).
 * - Single char Latin/Cyrillic/Greek: textLength 48, size 64.
 * - Two chars: textLength 64, size 52.
 * - Three chars: textLength 72, size 44.
 */
export function sealLayout(mark: SealMark): { targetLen: number; fontSize: number } {
  const isSquare = SQUARE_SCRIPTS.has(mark.script);
  const { charCount } = mark;
  if (charCount === 1 && isSquare) return { targetLen: 56, fontSize: 70 };
  if (charCount === 1) return { targetLen: 48, fontSize: 64 };
  if (charCount === 2) return { targetLen: 64, fontSize: 52 };
  return { targetLen: 72, fontSize: 44 };
}
