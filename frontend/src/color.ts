// Stable, distinct-ish colour per label via hashed hue (tuned for the dark background).
// Ported from the vanilla viewer; a shared cache keeps a label's colour consistent across the
// canvas, legend, and detail overlay.
//
// Two forms of the same colour: a CSS string for the DOM (legend, detail panel) and an RGB
// triple for the GPU vertex buffers. Both derive from the same hue, so they cannot drift.

const HUE_CACHE = new Map<string, number>()
const CSS_CACHE = new Map<string, string>()
const RGB_CACHE = new Map<string, [number, number, number]>()

const SATURATION = 0.62
const LIGHTNESS = 0.62

function hueFor(label: string): number {
  const hit = HUE_CACHE.get(label)
  if (hit !== undefined) return hit
  let h = 0
  for (let i = 0; i < label.length; i++) {
    h = (h * 31 + label.charCodeAt(i)) % 360
  }
  HUE_CACHE.set(label, h)
  return h
}

export function colorFor(label: string): string {
  const hit = CSS_CACHE.get(label)
  if (hit) return hit
  const c = `hsl(${hueFor(label)}, 62%, 62%)`
  CSS_CACHE.set(label, c)
  return c
}

/** The same colour as [`colorFor`], as `[r, g, b]` in 0-255 — for GPU colour attributes. */
export function rgbFor(label: string): [number, number, number] {
  const hit = RGB_CACHE.get(label)
  if (hit) return hit
  const rgb = hslToRgb(hueFor(label) / 360, SATURATION, LIGHTNESS)
  RGB_CACHE.set(label, rgb)
  return rgb
}

/** h, s, l in 0..1 → r, g, b in 0..255. */
function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  if (s === 0) {
    const v = Math.round(l * 255)
    return [v, v, v]
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q
  const channel = (t: number) => {
    if (t < 0) t += 1
    if (t > 1) t -= 1
    if (t < 1 / 6) return p + (q - p) * 6 * t
    if (t < 1 / 2) return q
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
    return p
  }
  return [
    Math.round(channel(h + 1 / 3) * 255),
    Math.round(channel(h) * 255),
    Math.round(channel(h - 1 / 3) * 255),
  ]
}
