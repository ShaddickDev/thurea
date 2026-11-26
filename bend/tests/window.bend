# Screen-time window spec (Bend).
#
# An independent model of `screen::Window` (bedtime): `inside` says whether a minute of
# the day falls in [start, end) going round the clock, and `change` is how many minutes
# until the next start or end (1..=1440). Both are written with modular distances
# instead of the branches the Rust uses, so the two implementations can disagree.
#
# For every test window, HVM checks all 1440 minutes in parallel (both halves of each
# range at once) and folds them into four fingerprints per start time. Rust computes the
# same from the real `Window` and compares row by row (crates/thurea-core/tests/bend_props.rs).
# STEP is replaced by the test; 20 gives 72 x 72 = 5184 windows, 7.5 million minutes.

def step():
  return STEP

def count():
  return 1440 / step()

def start_of(i):
  return (i * step() + (i % 3)) % 1440

def end_of(j):
  return (j * step() + (j % 5)) % 1440

def inside(s, e, m):
  return ((m + 1440 - s) % 1440) < ((e + 1440 - s) % 1440)

def dist(b, m):
  return ((b + 1439 - m) % 1440) + 1

def change(s, e, m):
  a = dist(s, m)
  b = dist(e, m)
  if a < b:
    return a
  else:
    return b

# Fingerprints over minutes lo..hi, split in halves so HVM runs them in parallel:
# minutes inside, sum of those minutes, sum of `change`, XOR of (change << 11 | minute).
def fp(s, e, lo, hi):
  if hi - lo == 1:
    c = change(s, e, lo)
    if inside(s, e, lo):
      return (1, lo, c, (c * 2048) | lo)
    else:
      return (0, 0, c, (c * 2048) | lo)
  else:
    mid = (lo + hi) / 2
    (a0, a1, a2, a3) = fp(s, e, lo, mid)
    (b0, b1, b2, b3) = fp(s, e, mid, hi)
    return (a0 + b0, a1 + b1, a2 + b2, a3 ^ b3)

def concat(a, b):
  match a:
    case List/Nil:
      return b
    case List/Cons:
      return List/Cons(a.head, concat(a.tail, b))

# One row: every end for start index i, folded so HVM's printed result stays small
# (it can't print 20,000 numbers). Sums wrap at 24 bits, as Bend numbers do.
def row(i, lo, hi):
  if hi - lo == 1:
    return fp(start_of(i), end_of(lo), 0, 1440)
  else:
    mid = (lo + hi) / 2
    (a0, a1, a2, a3) = row(i, lo, mid)
    (b0, b1, b2, b3) = row(i, mid, hi)
    return (a0 + b0, a1 + b1, a2 + b2, a3 ^ b3)

# Rows lo..hi, in parallel halves: [n, sum, change sum, xor] for each start.
def rows(lo, hi):
  if hi - lo == 1:
    (n, sm, sc, x) = row(lo, 0, count())
    return [n, sm, sc, x]
  else:
    mid = (lo + hi) / 2
    return concat(rows(lo, mid), rows(mid, hi))

def main():
  return rows(0, count())
