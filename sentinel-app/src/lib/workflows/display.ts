// Pure geometry/text helpers for the Display annotation nodes (Title / Paragraph / Markdown)
// rendered on the workflow canvas. No Svelte, no reactive state — safe to unit-test and reuse.
// Extracted from the editor component (#137).

/** Title font size (px) by the `size` config token. */
export const TITLE_SIZES: Record<string, number> = { sm: 18, md: 26, lg: 34, xl: 46 };

export function titleFontSize(size: unknown): number {
  return TITLE_SIZES[String(size ?? "lg")] ?? 34;
}

/** Auto-width of a Title card for its text at a given font size. */
export function titleWidth(text: string, fontPx: number): number {
  const t = text || "Title";
  return Math.max(120, Math.round(t.length * fontPx * 0.56) + 20);
}

export function titleHeight(fontPx: number): number {
  return fontPx + 18;
}

/** Strip the common inline markdown emphasis markers (SVG `<text>` can't style them). */
export function stripInline(s: string): string {
  return s
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/\*(.+?)\*/g, "$1")
    .replace(/`(.+?)`/g, "$1");
}

/** Wrap plain text into lines by an approximate char budget (SVG `<text>` does not wrap). */
export function wrapText(text: string, boxW: number, fontPx: number): string[] {
  const cpl = Math.max(6, Math.floor((boxW - 20) / (fontPx * 0.55)));
  const out: string[] = [];
  for (const para of String(text).split("\n")) {
    if (!para) {
      out.push("");
      continue;
    }
    let line = "";
    for (const word of para.split(/\s+/)) {
      let w = word;
      while (w.length > cpl) {
        if (line) {
          out.push(line);
          line = "";
        }
        out.push(w.slice(0, cpl));
        w = w.slice(cpl);
      }
      if (!line) line = w;
      else if ((line + " " + w).length <= cpl) line += " " + w;
      else {
        out.push(line);
        line = w;
      }
    }
    out.push(line);
  }
  return out;
}

export type MdLine = { text: string; size: number; bold: boolean; bullet: boolean; indent: number; y: number };

/**
 * Minimal, SVG-safe markdown: per-line heading (#/##/###) + bullet (-/*) styling, each carrying a
 * pre-computed baseline `y` (SVG `<text>` can't accumulate line height in a template). No
 * foreignObject (the editor deliberately avoids it for WebKit transform reasons).
 */
export function mdLines(text: string, boxW: number): MdLine[] {
  const raw: Omit<MdLine, "y">[] = [];
  for (const rawLine of String(text).split("\n")) {
    const line = stripInline(rawLine.replace(/\s+$/, ""));
    let m: RegExpMatchArray | null;
    if ((m = line.match(/^(#{1,3})\s+(.*)/))) {
      const size = m[1].length === 1 ? 18 : m[1].length === 2 ? 15 : 13;
      for (const w of wrapText(m[2], boxW, size)) raw.push({ text: w, size, bold: true, bullet: false, indent: 0 });
    } else if ((m = line.match(/^\s*[-*]\s+(.*)/))) {
      wrapText(m[1], boxW - 16, 12.5).forEach((w, i) => raw.push({ text: w, size: 12.5, bold: false, bullet: i === 0, indent: 16 }));
    } else {
      for (const w of wrapText(line, boxW, 12.5)) raw.push({ text: w, size: 12.5, bold: false, bullet: false, indent: 0 });
    }
  }
  let y = 14;
  return raw.map((l) => {
    y += l.size + 6;
    return { ...l, y: y - l.size + Math.round(l.size * 0.8) };
  });
}
