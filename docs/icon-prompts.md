# Icon generation prompts

Two prompts for an image generator (Gemini / Imagen, Midjourney, Flux, DALL·E 3 — works on all).
Generate the **app icon first**, pick a winner, then run the QR badge prompt with the
"same palette as the parent app icon" instruction so the two stay visually consistent.

Визуальный референс — фиолетовая аура со звёздочками и светящимися точками из приложения:
`StarHeroBackground` (фон героя на `UserPage`) и `StarCard` (карточка Star в сайдбаре /
лендинг-блок Star). Это **не космос**, не галактика, не тёмный сайфай — это мягкий
премиум-glow в фиолетово-лавандовых тонах с парящими звёздочками и пылинками-блёстками.
Иконка должна **сразу читаться как SoundCloud** — узнаваемое облачко, оранжевая основа.

---

## 1. App Icon — main application icon

```
Ultra-premium app icon for "SoundCloud Desktop", a high-fidelity desktop music player.
Squircle shape (iOS / visionOS rounded-square with continuous corner curvature, ~38%
corner radius), 1024×1024 hero icon viewed straight-on, perfectly centered, ample
padding so the icon reads at small sizes.

CORE IDENTITY (must be obvious): this is a SOUNDCLOUD client. The central motif is the
classic SoundCloud cloud silhouette — a soft, rounded, friendly cloud shape made of
overlapping bumps, exactly like the original SoundCloud logo. The cloud is the hero of
the icon, instantly recognizable at 32×32 px. Do NOT replace it with a waveform, vinyl,
note or abstract shape. Keep it a CLOUD.

PALETTE — keep SoundCloud's signature warm orange as the dominant fill of the cloud:
deep orange (#ff5500) → coral (#ff7a3d) → warm amber (#ffaa3d) gradient across the cloud
body, top-lit. The orange should be saturated and unmistakable — this is the SoundCloud
color, do not desaturate it.

BACKGROUND (inside the squircle, behind the cloud): soft premium violet aura, exactly
in the spirit of the app's `StarHeroBackground` and `StarCard` — NOT outer space, NOT a
galaxy, NOT a starfield. It is a calm radial glow:
- Base: deep plum / dark violet (#1a0f2e → #2a1850), very smooth, no banding.
- Aura: soft purple radial bloom from top-center (rgba(168,85,247,0.55)), a smaller
  lavender bloom from lower-left (rgba(139,92,246,0.35)), and a faint lilac glow from
  the right (rgba(192,132,252,0.3)). Same palette the app uses for the Star feature.
- A faint diagonal sheen of light lavender (rgba(192,132,252,0.1)) crossing the upper
  third, like a soft sparkle pass.

ACCENT PARTICLES (must match the app's Star ambience):
- 6–10 tiny five-point star glyphs scattered around the cloud, in lavender / purple /
  pale violet (HSL hues 250–320, 75–85% saturation, 70–80% lightness), each with a soft
  glow / drop-shadow, sizes varying from 4 to 14 px on the 1024 canvas. The stars look
  like small premium sparkles, not like astronomical stars.
- A few even smaller pinpoint dots of the same lavender hue with soft halos, like dust
  motes catching light. Sparse, calm, not busy.
- Optional one or two tiny amber/gold sparks near the cloud to tie back to the orange.

CLOUD STYLING:
- Soft inner light from above (cloud feels lit by a warm sun).
- Subtle inner shadow at the bottom of each bump for volume.
- A delicate hairline white inner stroke on the cloud silhouette (1px) for definition
  against the violet aura.
- Soft outer glow around the cloud in warm orange (rgba(255,85,0,0.35)) bleeding into
  the violet background — the orange and violet should kiss at the boundary, that
  contrast is the whole vibe.

SQUIRCLE FINISH:
- Hairline 1px white inner stroke around the squircle (Apple icon convention).
- Soft outer drop shadow, long and low-opacity, slightly violet-tinted.
- Very subtle frosted-glass top highlight (Apple-button gloss) across the upper third.

STYLE REFERENCES: original SoundCloud logo + iOS 18 / visionOS app icons + Apple Music
macOS icon for the gloss. NOT cosmic, NOT cyberpunk, NOT neon. Premium, friendly,
musical, soft.

MOOD: warm, inviting, premium, "this is SoundCloud, just nicer". Calm violet glow with
floating sparkles, friendly orange cloud at the center.

OUTPUT: 1024×1024 PNG with full alpha channel (transparent background outside the
squircle), ultra-high detail, sharp focus, no text, no letters, no logos, no watermark.
```

### Generator-specific tweaks

- **Gemini / Imagen 3**: append at the end — `studio render, sharp focus, ultra-detailed,
  transparent background, soft volumetric glow, no space / no galaxy / no starfield`.
- **Midjourney v6/v7**: append flags — `--ar 1:1 --style raw --stylize 600 --quality 2`.
  Add `--no text, words, letters, watermark, galaxy, nebula, space, stars background`.
- **Flux**: append — `studio render, sharp focus, ultra-detailed, soft violet aura,
  warm orange cloud, transparent background`.
- **DALL·E 3**: drop fancy render terms. Use `clean studio render, ultra-detailed, soft
  violet glow background, friendly SoundCloud cloud in warm orange`.

---

## 2. QR Code Centre Badge

```
Ultra-premium circular badge designed to sit at the center of a QR code. Perfect circle,
1024×1024 rendering with full alpha, exactly centered, no padding outside the circle (the
circle fills the entire frame edge-to-edge). The badge must read clearly even at 64×64 px
because it will be scaled down inside a QR code.

CRITICAL CONSTRAINTS:
- The cloud subject must occupy at least 75% of the circle. NO inner ring, NO inset
  background, NO "white halo around a tiny logo" — the artwork goes edge-to-edge.
- High contrast against both light AND dark surroundings, since QR codes use both.
- Strong silhouette — instantly recognizable as the SoundCloud cloud at thumbnail size.
- No text, no letters, no QR-pattern motifs — only the badge itself.

CORE SUBJECT: the classic SoundCloud cloud silhouette, friendly rounded bumps, filled
with the SoundCloud signature warm-orange gradient (#ff5500 → #ff7a3d → #ffaa3d), top-lit,
with a subtle inner shadow at the bottom for volume. Hairline 1px white stroke on the
cloud edge for crispness. Make it OBVIOUS this is SoundCloud.

BACKGROUND (inside the disc, behind the cloud): the same calm violet aura as the app's
`StarCard` and `StarHeroBackground` — NOT space, NOT a nebula. A soft purple radial
bloom from top-center (rgba(168,85,247,0.55)) over a deep plum base (#1a0f2e → #2a1850),
plus a smaller lavender glow from the lower edge (rgba(139,92,246,0.35)). Smooth, calm,
premium. A diagonal sparkle sheen across the upper third in pale lavender.

SPARKLES: 4–6 tiny five-point star glyphs in lavender / purple (HSL 250–320, ~80% sat,
~75% light), with soft drop-shadow glows, scattered around (not on top of) the cloud.
Plus a few small pinpoint dots with halos. Calm, sparse, not busy. One optional tiny
amber spark near the cloud to echo the orange.

DISC FINISH:
- Hairline 2px white inner stroke around the perimeter of the circle for definition.
- Apple-button-style top-light gloss highlight covering the upper third.
- Subtle inner shadow at the bottom curve for depth.
- Soft outer drop-shadow / halo around the disc in violet-magenta tones.
- Frosted-glass quality on the disc itself (visionOS-style), but the cloud reads as
  solid orange in front of the glass — never let the violet wash out the orange.

STYLE REFERENCES: original SoundCloud logo + Apple visionOS spatial badges + Apple Music
macOS app badge gloss. Premium, friendly, musical. Not cosmic, not cyberpunk, not neon.

MOOD: warm orange SoundCloud cloud floating on a calm violet sparkle aura. Reads as
"premium SoundCloud" at a glance.

OUTPUT: 1024×1024 PNG with full alpha (transparent outside the circle), photoreal glass
rendering, sharp focus, no text, no watermark, ultra-detailed.
```

### Generator-specific tweaks

- **Gemini / Imagen 3**: append — `circular composition, centered, edge-to-edge, studio
  render, transparent background, no galaxy, no space`.
- **Midjourney**: append — `--ar 1:1 --style raw --stylize 500 --quality 2 --no text,
  words, letters, watermark, galaxy, nebula, space, starfield`.
- **Flux**: append — `circular composition, centered, edge-to-edge, studio render,
  transparent background, soft violet aura, warm orange cloud`.
- **DALL·E 3**: append — `clean studio render, perfectly centered circular composition,
  friendly SoundCloud cloud in warm orange on calm violet glow, transparent background`.

---

## Workflow

1. **App icon first** — generate 4 variants, pick a winner.
2. **For consistency**, when running the QR-badge prompt, replace the BACKGROUND
   paragraph with: *"same calm violet sparkle aura as the parent app icon (attached /
   described above), same warm-orange SoundCloud cloud, scaled and reframed for a
   circle"*. If the generator supports image references / image-to-image, attach the
   chosen app icon as a reference.
3. **Transparent PNG** — Gemini / Midjourney / Flux can output alpha. DALL·E 3 cannot,
   so run results through https://remove.bg or Photopea magic-wand if needed.

## Sizes you'll need

### App icon

| Platform       | Sizes (PNG)                                                                                |
|----------------|--------------------------------------------------------------------------------------------|
| macOS `.icns`  | 1024, 512, 256, 128, 64, 32, 16                                                            |
| Windows `.ico` | 256, 128, 64, 48, 32, 16                                                                   |
| Linux          | 512, 256, 128, 64, 48                                                                      |
| Tauri          | drop the 1024 PNG into `src-tauri/icons/`, run `pnpm tauri icon` to auto-generate the rest |

### QR centre badge

| Where                            | Size                                          |
|----------------------------------|-----------------------------------------------|
| `desktop/src/assets/sc-logo.png` | 256×256 PNG (already wired into `QrCode.tsx`) |

Replace `desktop/src/assets/sc-logo.png` with the new badge — `QrCode.tsx` will pick it
up on next build, the offscreen-canvas compositor adds gloss + hairline automatically.
