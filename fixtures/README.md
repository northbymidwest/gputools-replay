# Fixtures

`reply_corpus_64x64.plist` - a real `GTReplayResponse` payload: the
`NSKeyedArchiver` plist returned by a 0..2000 streamRef sweep of
`corpus.gputrace` at a 64x64 region. 182 records, 3.1 MB.

Committed deliberately. It lets the reply parser - the plist walk, the 80-byte
`info` table, the payload offsets - be developed and tested with **no hardware,
no replay session, and no two-hour orphan risk**. Recorded from the prior
project, where it was the basis of most of the parsing tests.

Two properties worth knowing, both measured from this file:

- 182 records over only **180 distinct `resourceIndex` values** - one resource
  can be reached by more than one streamRef.
- Every record is tightly packed (`bytesPerRow == width * bytesPerPixel`) and
  has `depth == 1`. Do not assume either holds for an arbitrary capture; the
  parser should check rather than rely on it.

Note this is a 64x64 **resampled** reply, not natural size - see
`docs/HANDOFF.md` section 2.4.
