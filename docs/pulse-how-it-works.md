# SSPUR Pulse: how it works

[SSPUR Pulse](https://utkarshavardhana.github.io/sspur/pulse/) is a live dashboard of the world today: what people read on Wikipedia, which rockets are about to fly, where the ground shook, which new GitHub projects are rising fastest, what Hacker News was talking about on this date years ago, which AI papers came out, how hot it is in a hundred cities, and who has the day off. Everything behind it is written in SSPUR, and it costs nothing to run: no servers, no API keys, no paid services, and no AI at runtime.

The source is in [`pulse/`](https://github.com/utkarshavardhana/sspur/tree/main/pulse).

## The pieces

| Path | What it is |
|---|---|
| `pulse/lib` | The shared library, a package named `pulse`: HTTP through `curl` with timeouts, retries and a User-Agent, file helpers, time and number formatting, HTML, XML, URL and iCalendar escaping, a small XML extractor, the page shell, chart and map renderers, and the RSS and ICS writers |
| `pulse/<section>` | One package per board (`reading`, `launches`, `quakes`, `repos`, `hn`, `papers`, `weather`, `holidays`), each a single `main.ssp` that depends on `pulse` |
| `pulse/orbit` | Fetches the orbital elements of the satellites in orbit for the globe on the launches page |
| `pulse/overview` | Builds the front page from the eight `api/<section>.json` files, and the 404 page |
| `pulse/web` | Static assets: the stylesheet, a small vanilla script, the display font, icons, the land outline for maps |
| `pulse/fixtures` | Captured API responses, so tests and a first run work offline |
| `pulse/run.sh`, `pulse/test.sh`, `pulse/relock.sh` | Build the site, run every package's tests, re-pin the library after changing it |

## A run

`pulse/run.sh` runs the section programs one after another (the eight boards and `orbit`), then the overview. Each section does the same five steps:

1. Load its last good state from the data directory (`PULSE_DATA`).
2. Fetch, if it is due. Sections that change slowly wait: the space launch API is called at most once an hour, Hacker News history every six hours, and the holiday calendars once a week.
3. Decode the JSON or XML into plain SSPUR records, then check it. Each section filters out malformed items and refuses the whole fetch if too much is wrong; refined types and `pre` contracts on the renderers (magnitudes between -2 and 10, latitudes between -90 and 90, and so on) make sure nothing out of range reaches a page.
4. Settle: fresh data if the fetch worked, else the last good data with a note and the time it was last updated, else the bundled sample from `pulse/fixtures`. A run never publishes an empty board. `pulse.settle` does this for every section, and it has its own tests.
5. Render server-side HTML, a JSON API file (`api/<section>.json`), RSS feeds where they make sense, ICS calendars for launches and holidays, and a social card.

If a section program fails outright, `run.sh` runs it again with `PULSE_SKIP=1`, which renders from the stored state without fetching.

History is part of each section's state and is capped, typically at 30 days: daily Wikipedia rankings for sparklines and rank changes, star counts per repository for star velocity, daily paper counts by topic, and so on.

Here is part of the quakes board, the function that turns the USGS month feed into the daily counts of the 30 day chart. The [whole program](https://github.com/utkarshavardhana/sspur/blob/main/pulse/quakes/main.ssp) is about 160 lines.

```sspur
{{#include ../pulse/quakes/main.ssp:days}}
```

## Pages

Every page is complete without JavaScript: charts are drawn by SSPUR as HTML and SVG, with a visually hidden data table for screen readers. The script, under 10 KB compressed, only enhances them: it redraws a chart when you switch a metric or a time range, adds a crosshair and a tooltip that follow the pointer, a finger or the arrow keys, ticks countdowns and relative times, filters and sorts lists, and keeps each view in the URL so it can be shared. Long lists show their first 10 rows on phones and 20 on larger screens behind a "Show all" toggle, which also works without JavaScript. Easter eggs (try the Konami code, or type "sspur") live in a separate file that loads only when one is found. The chart specs it uses are the same records SSPUR rendered, embedded in the page as JSON.

The display face is Bricolage Grotesque (SIL Open Font License), cut down to one weight and the characters Pulse uses, about 18 KB.

## Your weather

The front page shows the weather where you are, next to your local time. Your approximate location comes from your IP via BigDataCloud's free client-side API; precise location is used only if you allow it and never leaves your browser except to fetch the weather from Open-Meteo. If neither works, Pulse uses the board city nearest to your browser's time zone. A place you choose is remembered in your browser's local storage, and the pin button next to the weather forgets it. Pulse has no analytics, no cookies and no server of its own, so nothing about your visit is stored anywhere else.

The launches page also draws the satellites in orbit on a globe. The SSPUR orbit program fetches CelesTrak's orbital elements every 6 hours, checks their ranges and age, groups them by operator and writes them as compact files; your browser propagates them with SGP4 (satellite.js) in a Web Worker, so the Earth turns at its true rate and every satellite moves at its real speed.

## Hosting

The Pages workflow (`.github/workflows/pages.yml`) runs on every push to `main`, every six hours, and by hand. It builds the docs and the playground, builds `sspur`, runs the Pulse tests, restores the snapshots from the `pulse-data` branch, runs `pulse/run.sh` into `docs/book/pulse`, and pushes the new snapshots back to `pulse-data` as a single commit, so that branch never grows a history. It uses only the built-in `GITHUB_TOKEN`, with `contents: write` for the data branch and `pages: write` and `id-token: write` to deploy. The pushes to `pulse-data` also count as repository activity, so GitHub does not pause the schedule after 60 quiet days.

## Running it yourself

```console
$ cargo build --release
$ pulse/test.sh
$ pulse/run.sh
$ python3 -m http.server -d pulse/out 8000
```

`PULSE_FIXTURES=pulse/fixtures` runs the whole pipeline offline on the captured responses, `PULSE_SECTIONS="quakes weather"` builds only some boards, `PULSE_FORCE=1` fetches even when a board is not due, and `PULSE_BACKFILL=30` lets the boards that build history from the past (Wikipedia and arXiv) fill more days per run.

Section programs are compiled to native code and cached, so a rerun only pays for the run itself. With `--interp` they give the same pages, a few times slower on the large feeds.

## Data sources

| Board | Source | License or terms |
|---|---|---|
| Reading | [Wikimedia REST API](https://wikimedia.org/api/rest_v1/), top pageviews | Article content CC BY-SA 4.0 |
| Launches | [The Space Devs Launch Library 2](https://thespacedevs.com/llapi) | Free tier, rate limited |
| Quakes | [USGS earthquake feeds](https://earthquake.usgs.gov/earthquakes/feed/) | Public domain |
| Repos | [GitHub search API](https://docs.github.com/en/rest/search) | GitHub terms |
| HN | [Hacker News Search by Algolia](https://hn.algolia.com/api) | Free API |
| Papers | [arXiv API](https://info.arxiv.org/help/api/) | Thank you to arXiv for use of its open access interoperability |
| Weather | [Open-Meteo](https://open-meteo.com/) | CC BY 4.0 |
| Holidays | [Nager.Date](https://date.nager.at/) | MIT |
