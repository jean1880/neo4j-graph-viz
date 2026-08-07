#!/usr/bin/env bash
# Smoke-test a running instance. Works against anything that serves the app — a local
# container, a dev server, or a deployed URL behind a proxy:
#
#   ./smoke.sh                        # defaults to http://127.0.0.1:8902
#   ./smoke.sh https://graph.example.com/
#
# Read-only: issues GETs and nothing else. Exits non-zero on the first failed check so it
# can gate a deploy.
set -euo pipefail

BASE="${1:-http://127.0.0.1:8902}"
BASE="${BASE%/}"

pass=0
fail=0
warn=0
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; pass=$((pass + 1)); }
bad()  { printf '  \033[31m✗\033[0m %s\n' "$1"; fail=$((fail + 1)); }
# Heuristics warn but never fail: a plausible-looking graph is a judgement call, and a deploy
# gate must not block on one.
huh()  { printf '  \033[33m!\033[0m %s\n' "$1"; warn=$((warn + 1)); }
note() { printf '    %s\n' "$1"; }

echo "Smoke-testing ${BASE}"

# --- 1. health ------------------------------------------------------------------------------
if [[ "$(curl -fsS -o /dev/null -w '%{http_code}' "${BASE}/healthz" 2>/dev/null)" == "200" ]]; then
  ok "/healthz responds 200"
else
  bad "/healthz did not respond 200"
fi

# --- 2. SPA + assets ------------------------------------------------------------------------
index="$(curl -fsS "${BASE}/" 2>/dev/null || true)"
if grep -q '<div id="app">' <<<"$index"; then
  ok "SPA index.html served"
else
  bad "SPA index.html not served"
fi

# Assets are referenced relatively, so resolve them against the base we were given — this is
# exactly what breaks first on a path-prefixed deployment.
asset="$(grep -oE 'assets/index-[A-Za-z0-9_-]+\.js' <<<"$index" | head -1 || true)"
if [[ -n "$asset" ]] &&
   [[ "$(curl -fsS -o /dev/null -w '%{http_code}' "${BASE}/${asset}" 2>/dev/null)" == "200" ]]; then
  ok "JS bundle resolves (${asset})"
else
  bad "JS bundle did not resolve${asset:+ (${asset})}"
fi

# --- 3. API ---------------------------------------------------------------------------------
body="$(curl -fsS "${BASE}/api/graph" 2>/dev/null || true)"
if [[ -n "$body" ]]; then
  ok "/api/graph responds"
else
  bad "/api/graph returned nothing"
fi

# --- 3b. node detail ------------------------------------------------------------------------
# Properties are served per node rather than in the graph payload, so the detail panel is only
# as good as this route. Check a real id resolves and that an unknown one 404s rather than
# 200-ing with an empty body — a detail endpoint that always succeeds is worse than none.
if [[ -n "$body" ]]; then
  node_id="$(python3 -c '
import sys, json
n = json.loads(sys.stdin.read()).get("nodes", [])
print(n[0]["id"] if n else "")
' <<<"$body" 2>/dev/null || true)"

  if [[ -n "$node_id" ]]; then
    detail="$(curl -fsS --get --data-urlencode "@-" /dev/null 2>/dev/null <<<"" || true)"
    # curl cannot urlencode a path segment, so encode it here (element ids contain ':').
    enc_id="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$node_id")"
    detail="$(curl -fsS "${BASE}/api/node/${enc_id}" 2>/dev/null || true)"
    if grep -q '"props"' <<<"$detail"; then
      ok "/api/node/{id} serves properties"
    else
      bad "/api/node/{id} did not return a props object for ${node_id}"
    fi

    code="$(curl -fsS -o /dev/null -w '%{http_code}' "${BASE}/api/node/definitely-not-a-node" 2>/dev/null || true)"
    if [[ "$code" == "404" ]]; then
      ok "/api/node/{id} 404s on an unknown id"
    else
      bad "/api/node/{id} returned ${code:-nothing} for an unknown id (expected 404)"
    fi
  else
    huh "could not read a node id from /api/graph — skipped the detail checks"
  fi
fi

# --- 4. compression -------------------------------------------------------------------------
enc="$(curl -fsS -o /dev/null -D- -H 'Accept-Encoding: gzip, br' "${BASE}/api/graph" 2>/dev/null |
       tr -d '\r' | awk -F': ' 'tolower($1)=="content-encoding"{print $2}')"
if [[ -n "$enc" ]]; then
  raw=$(wc -c <<<"$body")
  comp=$(curl -fsS -H 'Accept-Encoding: gzip, br' "${BASE}/api/graph" 2>/dev/null | wc -c)
  ok "compressed (${enc})"
  note "$(printf '%s bytes -> %s bytes' "$raw" "$comp")"
else
  # Compression is a deliberate config choice, not a defect: it is a clear win behind the Nginx
  # vhost and a pessimization over loopback, where view.sh turns it off. Warn, never fail.
  huh "no content-encoding negotiated"
  note "expected behind a proxy; over loopback view.sh sets GRAPH_COMPRESSION=0"
fi

# --- 5. payload sanity ----------------------------------------------------------------------
# Catches the failure mode that a 200 hides: a well-formed response describing a useless graph
# (no nodes, or every node collapsed onto a wrapper label because config is missing).
if [[ -n "$body" ]]; then
  summary="$(python3 -c '
import sys, json, collections
g = json.loads(sys.stdin.read())
n, l = g.get("nodes", []), g.get("links", [])
labels = collections.Counter(x["label"] for x in n)
unnamed = sum(1 for x in n if x.get("name") == "?")
top = labels.most_common(1)[0] if labels else ("-", 0)
share = (top[1] / len(n)) if n else 0
print(f"{len(n)}|{len(l)}|{len(labels)}|{unnamed}|{top[0]}|{share:.2f}")
' <<<"$body" 2>/dev/null || true)"

  if [[ -n "$summary" ]]; then
    IFS='|' read -r nodes links ltypes unnamed toplabel topshare <<<"$summary"
    if (( nodes > 0 && links >= 0 )); then
      ok "payload parses: ${nodes} nodes, ${links} links, ${ltypes} labels"
    else
      bad "payload has no nodes"
    fi
    if (( unnamed == 0 )); then
      ok "every node resolved a display name"
    else
      bad "${unnamed} node(s) fell back to \"?\" — check GRAPH_NAME_KEYS"
    fi
    # One label owning a large share of the graph often means a wrapper label is being shown
    # as the type — i.e. GRAPH_WRAPPER_LABELS is unset for this schema. It can also just be a
    # genuinely lopsided graph, so this warns rather than fails.
    if awk "BEGIN{exit !($topshare > 0.35)}"; then
      huh "label '${toplabel}' covers ${topshare} of all nodes"
      note "if that is a namespace/base label, set GRAPH_WRAPPER_LABELS"
    else
      ok "label distribution looks meaningful (largest '${toplabel}' at ${topshare})"
    fi
  else
    bad "payload is not valid JSON in the expected shape"
  fi
fi

echo
if (( fail > 0 )); then
  printf '\033[31mFAIL\033[0m  %d passed, %d failed, %d warned\n' "$pass" "$fail" "$warn"
  exit 1
fi
printf '\033[32mPASS\033[0m  %d checks, %d warning(s)\n' "$pass" "$warn"
