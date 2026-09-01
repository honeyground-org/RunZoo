# RunZoo

An animal runs across your menu bar as fast as your Mac is busy, and wears the
colour of how bad it is. Idle, it strolls in plain white; overloaded, it sprints
in red.

macOS only · Rust · minimal dependencies · 548KB bundle

<!-- screenshot goes here -->

## Install

**The quick way — no Rust, nothing to build.** Grab `RunZoo.zip` from the
[releases page](https://github.com/wangjacsi/RunZoo/releases), unzip it, and
drag `RunZoo.app` into Applications. The build is ad-hoc signed rather than
notarised, so macOS quarantines the download; clear it once:

```sh
xattr -dr com.apple.quarantine /Applications/RunZoo.app
open /Applications/RunZoo.app
```

**From source, on a Mac with nothing installed yet.** One command. It installs
the Xcode Command Line Tools and Rust if they are missing, builds, installs into
`/Applications` and launches:

```sh
curl -fsSL https://raw.githubusercontent.com/wangjacsi/RunZoo/main/tools/install.sh | bash
```

Already have a checkout? `./tools/install.sh`, and add `--login-item` to start it
at login. (By hand, that is System Settings → General → Login Items → add
`/Applications/RunZoo.app`.)

**Build only.** `./tools/bundle.sh` writes `dist/RunZoo.app`; add `--universal`
for a binary that runs on both Apple silicon and Intel.

## Seven animals

Cat · Dog · Rattlesnake · Squirrel · Rabbit · Elephant · Chicken

The menu bar icon is drawn as a silhouette. Colour is carried by the fill, so
the seven have to be told apart **by shape alone** — which is how you recognise
an animal anyway: the elephant by its trunk, the rabbit by its ears, the
squirrel by its tail, the chicken by its comb, the rattlesnake by its S and its
rattle.

At the same load each animal walks at its own tempo. The elephant ambles at
0.6x, the squirrel fusses at 1.4x (`src/animal.rs`).

## What it does

**Mini dashboard** — click the icon and the last 60 seconds of CPU, memory,
disk, network and battery each appear as a small graph.

**Pick what sets the speed** — click a row and that value drives the animal.
Pick battery and it runs more frantically the less charge is left.

**Severity in colour** — see below.

**Overload alert, with the culprit** — if the chosen value stays above 85% for
more than 30 seconds you get a notification naming the process using the most.
It clears once the value drops back under 70%, which stops it chattering at the
boundary.

## Severity in colour

Every source is normalised to 0..100, so one ramp turns any of them into "how
bad is this" and then into a colour. The colour shows up in three places:

- the **animal in the menu bar**, painted for whichever source is driving it;
- **every row of the dashboard**, coloured sample by sample — so a sparkline is
  a trace of severity over time, not one flat tint. A calm minute stays neutral
  and the spike in the middle of it stands out;
- the **palette itself**, where each entry is drawn as the whole gradient it
  would paint with, so you pick a ramp you can see rather than the name of a
  colour.

Pick the accent under **Severity colour** in the menu: red (the default),
orange, yellow, green, teal, blue, purple, pink, or monochrome to switch the
whole thing off and go back to the plain template icon.

```
severity = (load / 100) ^ 1.6          # 0 at rest, 1 at full
colour   = neutral + (accent - neutral) * severity
```

The exponent pushes the visible part of the ramp into the busy half: a machine
at 30% is only faintly tinted, one at 90% is unmistakable. It never goes flat,
so the colour always moves when the load moves.

**The calm end is white, in both appearances.** So the ramp reads as one thing:
"white to red", whichever menu bar you are on. It used to follow the menu bar —
white on dark, black on light, the way a template image does — and the cost of
dropping that is worth stating plainly: on a *light* menu bar an idle animal is
white on near-white and very nearly disappears until the load picks up. From
about half load it is legible again. If you are on a light menu bar and want an
idle animal you can see, pick the **Monochrome** accent: it hands the tinting
back to macOS and adapts to either appearance.

Severity is quantised to 32 steps, and the eight animation frames are repainted
only when the step changes — not on every tick, and never in monochrome mode,
where macOS does the tinting for us.

## Speed maths

The RunCat curve, multiplied by each animal's tempo.

```
speed          = max(1, load% / 5) x tempo
frame interval = 500 / speed  ms   (clamped to 33ms .. 500ms)
```

Load 0% → 500ms (2fps, a stroll) · load 100% → 33ms (30fps, a sprint).

The 30fps ceiling comes from measurement. At 45fps the animation cost more CPU
with no visible difference. **An app that measures load must not create it.**

## Measurements

| | |
|---|---|
| Bundle size | 548KB for one architecture (461KB binary) · 1.0MB universal, 517KB zipped |
| Memory | about 40MB resident |
| Idle CPU | about 2.5% (1.35% measuring + 1.2% animating) |
| CPU at full load | about 5.5% |

The two CPU rows were measured on the monochrome build. Colour was measured
against it side by side, both running for the same minute on the same busy
machine (system load 140–250%, so the animation was near its 30fps ceiling):
median 5.4% with colour against 4.95% without, twelve samples each. Half a
percentage point, at the edge of the run-to-run noise — the repaint only fires
when the severity step moves, and costs about 11k pixel writes when it does.

Two things worth writing down from measuring.

**The status item must be variable length.** Fix its length and macOS refits the
image every frame, which took CPU from 8% to 26% at 40fps (measured three times,
alternating). That is the opposite of the intuition, so it is a comment in
`src/main.rs`.

**Disk throughput agrees with `iostat`.** During a sustained write we read
7.5–8.0 GB/s against `iostat`'s 7.7–7.9 GB/s.

## Known limits

**Disk I/O from short-lived processes is missed.** Throughput is summed per
process, so anything born and dead between two refreshes (2 seconds) is never
observed. Writing 800MB with `dd` shows up as 35KB/s. Work made of many brief
processes, like a compile, reads lower than it is. Fixing it properly means
reading IOKit's block device statistics directly.

**Memory can disagree with Activity Monitor.** We use `sysinfo`'s definition
(total − available), which counts cache and compressed memory differently to
macOS, so it reads lower.

## Redrawing the animals

Sprites are assembled from shapes rather than punched in pixel by pixel: bodies,
heads and tails as ellipses and tapering curves, with the leg phase rotated each
frame.

```sh
python3 tools/gen_sprites.py   # refreshes assets/animals/* and src/sprites.rs
open assets/animals/_contact_sheet.png   # seven species x 8 frames, side by side
```

Change the numbers, run it again, look at the sheet. Pure Python, no
dependencies — the PNG encoder is in there too.

## Developer flags

```sh
runzoo --probe 10        # print measurements, with severity and colour, no GUI
runzoo --dump-menu       # print the menu as built, no clicking
runzoo --dump-tint       # print the whole colour ramp
runzoo --dump-sprites    # every animal across the ramp  → /tmp/runzoo_sprites.raw
runzoo --dump-spark-demo # sparklines from synthetic data → /tmp/runzoo_spark.raw

python3 tools/raw_to_png.py /tmp/runzoo_sprites.raw 240 252   # look at a dump
```

## Credit

Started from the idea in RunCat365 (Takuto Nakamura, Apache-2.0). What was taken
and what was not is spelled out in [NOTICE](NOTICE).
