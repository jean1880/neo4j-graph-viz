/**
 * Resolve an API path against the document the SPA was served from, so the app works unchanged
 * at `/` or under any proxy prefix (`/tools/graph/`).
 *
 * A root-absolute `/api/graph` would 404 on every path-prefixed deployment, and baking the
 * prefix in at build time would mean one image per mount point.
 */
export function apiUrl(path: string, params?: Record<string, string>): string {
  const url = new URL(path, document.baseURI)
  for (const [k, v] of Object.entries(params ?? {})) url.searchParams.set(k, v)
  return url.href
}
