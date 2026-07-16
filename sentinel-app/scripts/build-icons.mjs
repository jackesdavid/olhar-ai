// Re-cut the favicon / app-icon set from the OlharAI mark (rebrand #76, Phase 2f).
// Every artifact in sentinel-app/static/ was the old dot-field; this replaces them from
// assets/BRAND.md's geometry.
import sharp from "sharp";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const OUT = process.argv[2];
const INK = "#171717";
const PAPER = "#FAFAFA";
mkdirSync(OUT, { recursive: true });

// BRAND.md § The mark. Large variant (40 dots) above 40px; small (32 dots) below.
const markParts = (fill, { small = false } = {}) => {
  const ring = small
    ? { w: 3, dash: "0.1 9.717", o: 0.35, moon: 5 }
    : { w: 2, dash: "0.1 7.754", o: 0.3, moon: 4 };
  return `
  <mask id="ecl">
    <rect width="120" height="120" fill="white"/>
    <circle cx="71" cy="49" r="27" fill="black"/>
  </mask>
  <circle cx="60" cy="60" r="50" fill="none" stroke="${fill}" stroke-width="${ring.w}"
          stroke-dasharray="${ring.dash}" stroke-linecap="round" opacity="${ring.o}"/>
  <circle cx="95.4" cy="24.6" r="${ring.moon}" fill="${fill}"/>
  <circle cx="60" cy="60" r="31" fill="${fill}" mask="url(#ecl)"/>
  <circle cx="71" cy="49" r="6" fill="${fill}"/>`;
};

// Just the eye, cropped. Below ~24px the orbit's stroke and the moon are sub-pixel — they smear
// rather than shrink — and the disc only fills half the frame. The eye is what carries the
// identity, so the favicon is the eye.
const eyeParts = (fill) => `
  <mask id="ecl">
    <rect width="120" height="120" fill="white"/>
    <circle cx="71" cy="49" r="27" fill="black"/>
  </mask>
  <circle cx="60" cy="60" r="31" fill="${fill}" mask="url(#ecl)"/>
  <circle cx="71" cy="49" r="6" fill="${fill}"/>`;

const svg = (body, { box = "0 0 120 120" } = {}) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${box}">${body}</svg>`;

// The ink tile keeps the mark legible on any tab bar / home screen, light or dark — the same
// reason the old set used a dark tile. BRAND.md: rx 14 of 120.
const tiled = (parts, { rx = 14, box } = {}) =>
  svg(`<rect width="120" height="120" rx="${rx}" fill="${INK}"/>${parts}`, { box });

// Maskable: the OS applies its own mask, so NO baked corner radius (it would double-round), and
// the art must sit inside the 80% safe circle or Android crops it.
const maskable = (parts) =>
  svg(`<rect width="120" height="120" fill="${INK}"/>
       <g transform="translate(60,60) scale(0.8) translate(-60,-60)">${parts}</g>`);

const png = (src, size, file) =>
  sharp(Buffer.from(src)).resize(size, size).png().toFile(join(OUT, file));

// ICO carries PNG payloads: 6-byte ICONDIR + 16-byte entries + the PNGs.
async function ico(src, sizes, file) {
  const imgs = await Promise.all(
    sizes.map((s) => sharp(Buffer.from(src)).resize(s, s).png().toBuffer()),
  );
  const head = Buffer.alloc(6);
  head.writeUInt16LE(0, 0); head.writeUInt16LE(1, 2); head.writeUInt16LE(sizes.length, 4);
  let offset = 6 + 16 * sizes.length;
  const dir = sizes.map((s, i) => {
    const e = Buffer.alloc(16);
    e.writeUInt8(s >= 256 ? 0 : s, 0); e.writeUInt8(s >= 256 ? 0 : s, 1);
    e.writeUInt8(0, 2); e.writeUInt8(0, 3);
    e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
    e.writeUInt32LE(imgs[i].length, 8); e.writeUInt32LE(offset, 12);
    offset += imgs[i].length;
    return e;
  });
  writeFileSync(join(OUT, file), Buffer.concat([head, ...dir, ...imgs]));
}

const appIcon = tiled(markParts(PAPER));
const faviconTile = tiled(eyeParts(PAPER), { rx: 26, box: "25 25 70 70" });

// The SVG favicon: the eye, following the tab bar's theme.
writeFileSync(
  join(OUT, "favicon.svg"),
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="25 25 70 70">
  <style>
    .ink { fill: ${INK} }
    @media (prefers-color-scheme: dark) { .ink { fill: ${PAPER} } }
  </style>
  <mask id="ecl"><rect width="120" height="120" fill="white"/><circle cx="71" cy="49" r="27" fill="black"/></mask>
  <circle class="ink" cx="60" cy="60" r="31" mask="url(#ecl)"/>
  <circle class="ink" cx="71" cy="49" r="6"/>
</svg>`,
);
writeFileSync(join(OUT, "brand-mark.svg"), svg(markParts(INK)));

await Promise.all([
  png(faviconTile, 16, "favicon-16.png"),
  png(faviconTile, 32, "favicon-32.png"),
  png(appIcon, 180, "apple-touch-icon.png"),
  png(appIcon, 192, "icon-192.png"),
  png(appIcon, 512, "icon-512.png"),
  png(maskable(markParts(PAPER)), 192, "icon-192-maskable.png"),
  png(maskable(markParts(PAPER)), 512, "icon-512-maskable.png"),
  ico(faviconTile, [16, 32, 48], "favicon.ico"),
]);
console.log("✓ icon set re-cut from the OlharAI mark");
