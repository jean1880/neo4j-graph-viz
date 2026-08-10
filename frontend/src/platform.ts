/**
 * How the command modifier is *written* on this platform — `⌘` on Apple, `Ctrl` elsewhere.
 *
 * Presentation only. The handlers deliberately keep accepting **either** modifier (see
 * `TypeLegend`), because on macOS ctrl-click is also a right-click and a Mac user may reach for
 * either key; narrowing the binding would break a habit rather than fix one. What was actually
 * wrong was the *label*, which told every Mac user to press a key their OS reserves for the
 * context menu.
 */

/**
 * Apple detection, best available source first.
 *
 * `navigator.platform` is deprecated but remains the only synchronous signal every browser
 * agrees on; `userAgentData.platform` is preferred where it exists. The user-agent string is the
 * last resort, and matches iPadOS too — it reports as a Mac, which for a key label is exactly
 * the behaviour we want anyway.
 */
function detectApple(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const platform = nav.userAgentData?.platform ?? nav.platform ?? ''
  if (platform) return /mac|iphone|ipad|ipod/i.test(platform)
  return /mac|iphone|ipad|ipod/i.test(nav.userAgent)
}

/** Evaluated once — the platform cannot change mid-session. */
export const isApple = detectApple()

/** The modifier as it should appear in tooltips and hints. */
export const MOD_LABEL = isApple ? '⌘' : 'Ctrl'
