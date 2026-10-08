import type { Deliverable } from "./types.js";

const NUMBERED = /^\s*(\d+)[.)]\s+(.+?)\s*$/;
const BULLET = /^\s*[-*]\s+(?:\[[ xX]\]\s+)?(.+?)\s*$/;
const HEADING = /^\s*#{1,6}\s/;

/**
 * Extract the deliverables list from a milestone spec. Numbered items win;
 * bullet items are the fallback. Indented or unbroken continuation lines are
 * folded into the preceding item.
 */
export function parseDeliverables(spec: string): Deliverable[] {
  const lines = spec.replace(/\r\n?/g, "\n").split("\n");
  const numbered = collect(lines, NUMBERED, 2);
  if (numbered.length > 0) return numbered;
  return collect(lines, BULLET, 1);
}

function collect(lines: string[], pattern: RegExp, group: number): Deliverable[] {
  const items: Deliverable[] = [];
  let open = false;
  for (const line of lines) {
    const m = line.match(pattern);
    if (m) {
      items.push({ id: items.length + 1, text: m[group].trim() });
      open = true;
      continue;
    }
    if (line.trim() === "" || HEADING.test(line)) {
      open = false;
      continue;
    }
    if (open && items.length > 0 && !NUMBERED.test(line) && !BULLET.test(line)) {
      items[items.length - 1].text += " " + line.trim();
    }
  }
  return items;
}
