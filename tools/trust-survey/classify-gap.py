#!/usr/bin/env python3
"""Classify a Trust survey JSON into user-logic vs compiler-derived boilerplate.

The raw obligation count conflates two very different things:
  - USER LOGIC: hand-written functions — the real "verify clean" target.
  - DERIVED BOILERPLATE: `#[derive(Debug/Clone/PartialEq/...)]`-generated impls.
    Verifying these proves the *compiler's* codegen, not the user's code; on
    orca-core every derived `Debug::fmt` obligation is an identical `&dyn Trait`
    Unsize `unsupported_mir`. They dominate the unknown count and mask the real
    frontier.

Usage: classify-gap.py <survey.json>   (defaults to newest /tmp/trust-survey/orca-core-*.json)
"""
import json, sys, re, collections, glob, os

DERIVE_PATS = [
    r'as std::fmt::Debug>::fmt',
    r'as std::fmt::Display>::fmt',
    r'as std::clone::Clone>::clone',
    r'as std::cmp::PartialEq>::eq',
    r'as std::cmp::PartialOrd>::partial_cmp',
    r'as std::cmp::Ord>::cmp',
    r'as std::hash::Hash>::hash',
    r'as std::default::Default>::default',
    r'as std::cmp::Eq>',
]
_derive_re = re.compile('|'.join(DERIVE_PATS))


def is_derived(name: str) -> bool:
    return bool(_derive_re.search(name))


def status(ob: dict):
    oc = ob.get('outcome')
    return (oc.get('status') if isinstance(oc, dict) else oc) or 'NA'


def reason_tag(ob: dict) -> str:
    desc = ob.get('description', '') or ''
    m = re.search(r'unsupported MIR `([^`]+)`', desc)
    if m:
        return f'unsupported_mir: {m.group(1)}'
    oc = ob.get('outcome')
    if isinstance(oc, dict) and oc.get('reason'):
        return str(oc['reason'])[:60]
    return (ob.get('kind') or desc.split(':')[0])[:50]


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else None
    if not path:
        cands = sorted(glob.glob('/tmp/trust-survey/orca-core-*.json'), key=os.path.getmtime)
        path = cands[-1] if cands else None
    if not path:
        print('no survey json found', file=sys.stderr); sys.exit(2)
    data = json.load(open(path))
    fns = data['functions']
    buckets = {'user': collections.Counter(), 'derived': collections.Counter()}
    fn_kind = collections.Counter()
    user_reasons = {'unknown': collections.Counter(), 'failed': collections.Counter()}
    for fn in fns:
        name = fn['function']
        kind = 'derived' if is_derived(name) else 'user'
        fn_kind[kind] += 1
        for ob in fn.get('obligations', []):
            st = status(ob)
            buckets[kind][st] += 1
            if kind == 'user' and st in user_reasons:
                user_reasons[st][reason_tag(ob)] += 1

    print(f'survey: {os.path.basename(path)}')
    for kind in ('user', 'derived'):
        b = buckets[kind]; tot = sum(b.values())
        print(f'\n=== {kind} logic: {fn_kind[kind]} fns, {tot} obligations ===')
        for s, c in b.most_common():
            print(f'  {c:5} {s}')
    u = buckets['user']
    gap = u['unknown'] + u['failed']
    print('\n=== HEADLINE — user-logic gap (the real "verify clean" target) ===')
    print(f'  proved {u["proved"]} / unknown {u["unknown"]} / failed {u["failed"]} '
          f'/ design_req {u["design_requirement"]}')
    print(f'  user-logic unprovable (unknown+failed) = {gap}')
    print(f'  derived-boilerplate unknown (separate &dyn-Trait Unsize lever) = {buckets["derived"]["unknown"]}')
    for st in ('unknown', 'failed'):
        print(f'\n  user-logic {st} by reason:')
        for r, c in user_reasons[st].most_common(10):
            print(f'    {c:4} {r}')


if __name__ == '__main__':
    main()
