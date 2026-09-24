// Class strings shared by the HUD's collapsible sections (TypeLegend, LayoutSettings), so the
// two disclosures cannot drift apart.

/** The `<details>` wrapper; `group` lets the chevron read the open state. The open/close
 *  height tween is progressive: only engines with ::details-content animate, the rest snap. */
export const section =
  'group mt-3 border-t border-border-strong pt-2 motion-safe:details-content:overflow-clip motion-safe:details-content:[block-size:0] motion-safe:details-content:transition-[block-size,content-visibility] motion-safe:details-content:transition-discrete motion-safe:details-content:duration-200 motion-safe:open:details-content:[block-size:auto]'

/** `<summary>` with the native marker replaced by the chevron. */
export const summary =
  'flex cursor-pointer list-none items-center gap-2 rounded-sm py-0.5 select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary [&::-webkit-details-marker]:hidden'

/** CSS-triangle chevron that rotates when the section opens. */
export const chevron =
  'size-0 flex-none border-y-4 border-s-5 border-y-transparent border-s-current text-fg-dim transition-[rotate] duration-160 group-open:rotate-90 motion-reduce:transition-none'

export const heading = 'flex-1 text-sm font-semibold tracking-wider text-fg-muted uppercase'
export const tip = 'mt-2 text-xs text-fg-dim'
export const inlineLink = 'cursor-pointer text-primary underline'
