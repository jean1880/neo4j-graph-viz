// Stable, distinct-ish colour per label via hashed hue (tuned for the dark background).
// Ported from the vanilla viewer; a shared cache keeps a label's colour consistent across the
// canvas, legend, and detail overlay.
const cache = new Map<string, string>()

export function colorFor(label: string): string {
  const hit = cache.get(label)
  if (hit) return hit
  let h = 0
  for (let i = 0; i < label.length; i++) {
    h = (h * 31 + label.charCodeAt(i)) % 360
  }
  const c = `hsl(${h}, 62%, 62%)`
  cache.set(label, c)
  return c
}
