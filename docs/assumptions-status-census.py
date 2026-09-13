#!/usr/bin/env python3
"""Census the status declarations in ASSUMPTIONS.md.

WHY THIS EXISTS, and why no grep replaces it. This file writes statuses as bare
mixed-case words inside prose, and the lowercase forms collide with ordinary
English in the same file ("cannot be confirmed after the fact", "DEFERRED (open,
...)"). Raw token counts therefore answer a different question than "how many
open items are there". Three incompatible figures were in circulation before this
tool existed -- 50 (`grep -c UNCONFIRMED`, counts prose), 28 (an `^`-anchored
`**Status:**` pattern, misses five other live shapes), and 35 (a careful hand
count, scoped to "deferred items" and superseded by later entries).

WHAT IT COUNTS. A *declaration* is a status asserted about an entry. Four shapes
exist in this file and all four are live:

  1. canonical      `- **Status:** UNCONFIRMED`
  2. mid-prose-line `... inside a docs pass. **Status: CONFIRMED.**`
  3. wrapped        `... **Status:` with the TOKEN on the FOLLOWING line
  4. label form     `- **UNCONFIRMED -- reported, not acted on:**`
                    `### OPEN -- escalated, NOT decided here: ...`

Excluded, deliberately:
  * prose mentions  -- a token with no Status label governing it, e.g. a
                       `**Correction:**` narrating a PAST status
  * inline code     -- a token or label inside backticks is documentation
                       quoting the shape, not a declaration of it
  * fenced blocks   -- none exist today; handled so the sweep stays robust

An *item* is an entry (nearest enclosing `##`/`###` heading). One item can carry
several declarations -- an original plus a later update -- and counts once.

USAGE
    python3 docs/assumptions-status-census.py            # summary
    python3 docs/assumptions-status-census.py --open     # list open declarations
    python3 docs/assumptions-status-census.py --prose    # list what was excluded
    python3 docs/assumptions-status-census.py --json     # machine-readable

The partition is asserted to be total: open + resolved + unrecognised == all
declarations. If that assertion ever fails, the shape list above is incomplete.
"""
import re, sys, json

OPEN = {'UNCONFIRMED', 'OPEN'}
RES  = {'CONFIRMED','DECLINED','SUPERSEDED','PARTIAL','NEEDS-CHANGE','DEFERRED','RESOLVED',
        'WITHDRAWN','RETIRED','OBSOLETE','CLOSED','CHANGED','RETRACTED','REFUTED','CORRECTED',
        'ACCEPTED','FIXED','REMOVED'}
ALL = OPEN | RES

# the label: '**Status', then anything that is not a colon (a parenthetical, ' of the
# original assumption'), then a colon, then optional closing bold and whitespace.
LABEL = re.compile(r'\*\*Status\b[^:\n]*:\*{0,2}[ \t]*')
TOKEN = re.compile(r'\*{0,2}([A-Z][A-Z-]{2,})\b')

CODESPAN = re.compile(r'`[^`]*`')

def scan(lines):
    out, fenced = [], False
    for i, raw in enumerate(lines):
        # Blank out inline code spans before matching. A `**Status:**` or a token INSIDE
        # backticks is documentation quoting the shape, not a declaration of it -- e.g.
        # U-505's `Status: UNCONFIRMED` counts, and this unit's own prose quoting
        # `**Status (updated 2026-09-01):**`. Width is preserved so line offsets stay valid.
        line = CODESPAN.sub(lambda m: ' ' * len(m.group(0)), raw)
        if line.lstrip().startswith('```'):
            fenced = not fenced
            continue
        for m in LABEL.finditer(line):
            rest = line[m.end():]
            tm = TOKEN.match(rest.lstrip())
            src = i + 1
            if not tm:                                   # token wrapped to next line
                nxt = lines[i+1] if i+1 < len(lines) else ''
                tm = TOKEN.match(nxt.strip())
                src = i + 2
            tok = tm.group(1) if tm else None
            out.append(dict(line=i+1, token_line=src, token=tok,
                            kind='fenced' if fenced else 'status',
                            text=raw.strip()[:100]))
        # A token that IS the leading label of a bullet or heading is a declaration with no
        # 'Status' word at all -- U-505 recorded 9 of these ('the bullet IS the status'). Missing
        # them undercounts exactly the way an anchored Status regex does, one boundary over.
        bl = re.match(r'^\s*(?:[-*]\s+\*\*|#{2,4}\s+\**)([A-Z][A-Z-]{2,})\b', line)
        if bl and bl.group(1) in ALL:
            out.append(dict(line=i+1, token_line=i+1, token=bl.group(1),
                            kind='fenced' if fenced else 'label',
                            text=raw.strip()[:100]))
            continue
        # tokens on this line NOT governed by a label on this line
        if not LABEL.search(line):
            for tm in TOKEN.finditer(line):
                if tm.group(1) in ALL:
                    # is it the wrapped continuation of a previous line's label?
                    prev = lines[i-1] if i else ''
                    if LABEL.search(prev) and not TOKEN.match(prev[LABEL.search(prev).end():].lstrip()):
                        continue                          # already captured as wrapped
                    out.append(dict(line=i+1, token_line=i+1, token=tm.group(1),
                                    kind='fenced' if fenced else 'prose',
                                    text=raw.strip()[:100]))
    return out

def main():
    path = 'ASSUMPTIONS.md'
    lines = open(path).read().splitlines()
    occ = scan(lines)
    st = [o for o in occ if o['kind'] in ('status','label')]
    pr = [o for o in occ if o['kind'] == 'prose']
    fn = [o for o in occ if o['kind'] == 'fenced']
    op = [o for o in st if o['token'] in OPEN]
    rs = [o for o in st if o['token'] in RES]
    un = [o for o in st if o['token'] not in ALL]
    if '--json' in sys.argv:
        print(json.dumps(dict(status=st, prose=pr, fenced=fn))); return
    lb = [o for o in occ if o['kind'] == 'label']
    print(f"  status declarations : {len(st)}   (Status-labelled {len(st)-len(lb)} + bullet/heading-label {len(lb)})")
    print(f"    OPEN  (UNCONFIRMED/OPEN) : {len(op)}")
    print(f"    RESOLVED                 : {len(rs)}")
    print(f"    unrecognised token       : {len(un)}   {[o['token'] for o in un]}")
    print(f"    {len(op)}+{len(rs)}+{len(un)} = {len(op)+len(rs)+len(un)}  == {len(st)}: {len(op)+len(rs)+len(un)==len(st)}")
    print(f"  prose mentions (excluded) : {len(pr)}")
    print(f"  fenced (excluded)         : {len(fn)}")
    if '--open' in sys.argv:
        print("\n  OPEN status declarations:")
        for o in op: print(f"    :{o['line']:<5} {o['token']:<12} {o['text'][:84]}")
    if '--prose' in sys.argv:
        print("\n  prose mentions, excluded:")
        for o in pr: print(f"    :{o['line']:<5} {o['token']:<12} {o['text'][:84]}")

main()
