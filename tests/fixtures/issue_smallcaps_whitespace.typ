// Test: whitespace-only runs must not be marked smallcaps (upstream #23).
// `#smallcaps[A] #smallcaps[B]` used to collapse into a single smallCaps
// run "A B" because the space run matched via `str::contains("")`.

#smallcaps[A] #smallcaps[B]
