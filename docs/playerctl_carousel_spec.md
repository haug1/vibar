# Playerctl Fixed-Width Carousel Spec

## Goal

Keep the `playerctl` module from occupying too much width on the bar when metadata text is long, while still allowing the full text to be read.

## Desired Config

Add/keep a `playerctl` option:

- `fixed-width`

Example:

```jsonc
{
  "type": "playerctl",
  "format": "{status_icon} {artist} - {title}",
  "fixed-width": 40,
}
```

## Required Behavior

1. Fixed visual width:

- When `fixed-width` is set, module width is visually bounded and must not grow beyond it with long titles.

2. Smooth marquee/carousel:

- If rendered text exceeds the fixed width, scroll horizontally so the full text becomes readable.
- Scrolling should look stable with proportional fonts (no width jitter).

3. Tooltip:

- Hover tooltip should show the full untruncated rendered text.

4. Backward compatibility:

- If `fixed-width` is unset, keep current `playerctl` behavior.

## UX Expectations

- Brief pause at loop start/restart is acceptable.
- Scroll speed should be readable (not too fast).
