# kage-models

The curated provider/model catalog for [kage](https://github.com/QaidVoid/kage).

kage needs one document describing every provider it supports and every
tool-calling model each serves. No single upstream publishes that. This
repo reads the five sources that do between them, and publishes one
`catalog.json` in models.dev's shape. kage reads that one file, so its
own code needs to know none of the upstreams exist.

## Why it exists

kage used to build its catalog itself, in `cargo xtask refresh-models`.
That command fetched five upstreams and refused to complete until every
disagreement with a committed hand-maintained fixture had been fixed by
hand. Command Code publishing a new model was enough to break it:

```
xtask: refresh-models failed: Command Code model drift, update
xtask/fixtures/manual.json: retired upstream: inclusionai/ling-3.0-flash-sante:free;
new upstream: Qwen/Qwen3.8-Max-Prime; new upstream: google/gemini-4-argon; ...
```

Updating a model list meant editing a 78 KB JSON file in kage's source
tree. This repo inverts that: the upstreams decide what exists, and the
editing that remains is a few lines per model in a small TOML file.

## Sources

| What it decides | Source |
| --- | --- |
| Providers, models, context, pricing | [models.dev](https://models.dev/api.json) |
| Which Kenari models exist, what they serve | `https://kenari.id/v1/models` |
| Which Kenari models a subscription covers | `https://kenari.id/api/plans` |
| Which Command Code models exist, what they serve | `https://api.commandcode.ai/provider/v1/models` |
| Command Code reasoning effort levels | the `command-code` npm package's generated `models.md` |
| Provider logos | `https://models.dev/logos/<id>.svg` |

models.dev alone is not enough. Its Kenari entry lists the pay-as-you-go
catalog, which is far wider than what Kenari's subscription plans serve.

## Layout

```
catalog/generated/<provider>.toml        upstream data, rewritten each run
catalog/generated/<provider>/NNN.toml    the same, sharded past 40 models
catalog/curated/<provider>.toml          the fields no upstream publishes
logos/<provider>.svg                     provider logos, one file each
```

The generated files are the review surface. A new Command Code model
arrives as one stanza in one small file:

```toml
[[models]]
id = "z-ai/glm-5.3-flash"
name = "GLM-5.3 Flash"
context = 1050000
output = 131072
input = ["text"]
thinks = true
toggle = true
efforts = ["low", "high", "max"]
release_date = "2026-08-26"
```

A provider with more than 40 models is split into numbered shards, so no
single file becomes a diff nobody reads. OpenRouter is the only one that
needs it today.

The curated files hold exactly two fields, because no upstream publishes
them and both are observed rather than derived:

- `release_date`, for Command Code. The model endpoint's `created` is a
  fetch timestamp, not a release date.
- `interleaved`, the field a model reads its reasoning back from during
  a tool loop. models.dev carries `null` for rows that demonstrably use
  `reasoning_content`.

A curated entry for a model the upstream dropped is reported and removed,
so the file cannot grow back a model that is no longer served.

## Logos

kage's settings screens draw a provider's logo. It used to fetch one
from models.dev at runtime, which made every window open a round trip to
a third party and left the web build unable to show any at all. The
logos are snapshotted here instead, one SVG per provider, and published
beside the catalog at `/logos/<id>.svg`.

Two replies are rejected rather than stored:

- **The generic glyph.** models.dev answers an id it has no logo for with
  HTTP 200 and a placeholder sparkle, so an unchecked fetch would commit
  a file that looks like a logo and draws as one.
- **Anything that is not an SVG.** A provider whose domain is parked
  answers with an HTML page.

`commandcode-claude` and `kenari-claude` read their parent provider's
logo, because models.dev files them under no name of their own. A
provider with no logo gets no file, and kage falls back to its own glyph;
the run names it in a warning rather than passing over it.

## Publishing

`generate` writes the TOML files and the logos. `bundle` turns the TOML
into the single models.dev-shaped document kage fetches and copies the
logos and the routing config into `public/`. The scheduled workflow
deploys that directory to Cloudflare Pages, where kage reads the catalog
from `https://models.kage.qaidvoid.dev/` and its logos from
`https://models.kage.qaidvoid.dev/logos/<id>.svg`. The
deploy step verifies both the served catalog and a served logo before
reporting success, so a truncated catalog or a missing image directory
never goes live silently.

The bundle is read back from the committed TOML rather than from the run
in memory, so a hand correction to a file reaches kage even if a
generator bug would have reverted it.

To deploy by hand:

```sh
cargo run -p kage-models -- generate
mkdir -p public && cargo run -p kage-models -- bundle > public/models.json
npx wrangler pages deploy public --project-name kage-models
```

No `--branch` flag: on a Direct Upload project wrangler treats any
`--branch` value as a request for a *preview* deployment, which leaves
the custom domain serving the previous file while the run reports
success.

The workflow needs two repository secrets: `CF_API_TOKEN` and
`CF_ACCOUNT_ID`.

## Use

```sh
cargo run -p kage-models -- generate   # fetch, rebuild the TOML and logo files
cargo run -p kage-models -- check      # fail if the files are stale (CI gate)
cargo run -p kage-models -- bundle     # print the document kage fetches
cargo test                             # the pipeline over recorded fixtures
```

Any upstream can be pointed elsewhere, which is how the tests run
offline:

```sh
cargo run -p kage-models -- generate --kenari-models-url file:///tmp/kenari.json
```

## How disagreements resolve

1. **A model only the endpoint lists, or only the CLI catalog lists, is
   left out and reported.** The two publish on their own schedules.
   Publishing it anyway would mean guessing its reasoning shape, and kage
   would offer it as non-thinking while silently dropping its thinking.
2. **A model filed under the wrong wire format is an error.** If the
   endpoint serves it on `/messages` and the CLI catalog files it under
   Chat Completions, no later run fixes that.
3. **An effort level kage cannot represent is an error.** The CLI catalog
   publishing `turbo` is a real gap in kage, and a catalog that papers
   over it would hide the gap.

A run that fails leaves the previous files in place. A slightly stale
catalog is better than an empty one.

## Dependencies

The crate depends on `kage-provider` by git revision, so it reads the
catalog through kage's own parser and cannot publish a document kage
would misread. Bumping that revision is part of adopting a kage change
that touches the catalog schema.