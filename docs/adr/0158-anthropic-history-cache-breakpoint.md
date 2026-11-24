# ADR 0158 — Rolling cache breakpoint on the Anthropic conversation history

## Status

Accepted

Betrifft: Issue #1308, Spec 0064 Part 7, ADR 0055 §1, Spec 0081

## Problem

ADR 0055 §1 left `messages` without a breakpoint. In a measured session
(216 requests, ~3.5 h) cache reads stayed at ~13.7k tokens (system + tools)
while uncached input grew from 5k to ~130k per request: about 5.2 M uncached
input tokens against 0.83 M cache reads. Every automatic follow-up round
resent the whole history uncached.

## Decision

1. **Breakpoint on the last non-empty block of the last message** in every
   main-chat request (system + last tool + history = 3 of 4 allowed).
   Side calls (`max_tokens_hint` set: second opinion, title, note
   suggestion, compaction) get none: their history is not resent by a
   follow-up request, so a cache write (1.25x) would never be read.
2. **Second history breakpoint** 20 blocks before the last one when the
   history has more than 20 blocks. Anthropic looks back only ~20 blocks from
   a breakpoint for an earlier entry; the extra marker keeps the previous
   entry findable after a round that added many blocks (4 of 4 at most).
3. **Prefix stability.** `content` is always a block array (previously a
   single text part was a plain string that turned into an array when a
   later same-role message was merged in, changing earlier bytes). Markers
   are not part of the cached prefix, so moving them does not break hits.
4. **Never mark empty or whitespace-only text blocks** (Spec 0081); the
   marker falls back to the previous eligible block, or is omitted.
5. **TTL stays at the default 5 minutes**; follow-up rounds are seconds
   apart. A pause above 5 minutes costs one full write, still cheaper than
   uncached input every round. A 1-hour TTL (2x write) is a separate
   measurement.

## Cache-breaking spots reviewed

- Plain string -> array switch on merge: fixed (3).
- Redaction (`RedactedContext::seal`) and fencing are pure functions of the
  stored entry and run on every send; no timestamps, counters or random ids
  enter history content. Output for unchanged entries is identical.
- Compaction rewrites the history once by design (Spec 0057); the next
  request is a cache miss from the changed point, accepted.
- The first message with date/banner state is the stable prefix start
  (Spec 0064 Part 2), unchanged.
- Other providers are unchanged (OpenAI caches automatically).

## Cost

Writes cost 1.25x input, reads 0.1x. Measured effect with a real key: to be
reported in the pull request (needs a manual check; no key in CI).

## Consequences

ADR 0055 §1 "no breakpoint on messages" is superseded. No security impact:
the bytes sent are unchanged apart from the block-array form.
